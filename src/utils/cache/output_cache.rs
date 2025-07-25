use std::io::stdout;

use matrix_sdk::{
    deserialized_responses::{TimelineEvent, TimelineEventKind}, ruma::events::{
        room::message::{MessageType, RoomMessageEvent, RoomMessageEventContent}, MessageLikeEvent, OriginalMessageLikeEvent
    }, stream::StreamExt, Client
};
use promkit::crossterm::{ExecutableCommand, cursor};
use tokio::{fs::DirBuilder, sync::mpsc::Receiver};
use tracing::Level;

use crate::utils::traits::{FilePath, ValidEvent, ValidMediaEvent};

#[derive(Clone)]
pub struct FileCache {
    user: String,
    messages: Vec<TimelineEvent>,
}

impl FileCache {
    pub fn new(user: String) -> Self {
        Self {
            user,
            messages: Vec::new(),
        }
    }

    pub async fn update_messages(
        &mut self,
        mut msg_rx: Receiver<Vec<TimelineEvent>>,
        mut write_rx: Receiver<bool>,
        client: &Client,
    ) -> anyhow::Result<()> {
        let mut buf_events = 0;
        let mut process_handle = tokio::task::JoinSet::new();

        while let Some(mut new_events) = msg_rx.recv().await {
            buf_events += &new_events.len();
            stdout().execute(cursor::MoveDown(3))?;
            println!("(cache) Saving {} events", buf_events);
            stdout().execute(cursor::RestorePosition)?;

            self.messages.append(&mut new_events);

            if let Ok(_) = write_rx.try_recv() {
                buf_events = 0;
                let mut cached_events = Vec::new();
                std::mem::swap(&mut cached_events, &mut self.messages);

                let cache = self.clone();
                let client = client.clone();
                process_handle.spawn(async move {
                    cache.filter_types(cached_events, &client).await
                });
            }
        }

        while let Some(res) = process_handle.join_next().await {
            match res {
                Ok(_) => (),
                Err(err) => tracing::event!(Level::ERROR, "Event process/write task error: {err}")
            }
        }

        anyhow::Ok(())
    }

    async fn filter_types(
        &self,
        cached_events: Vec<TimelineEvent>,
        client: &Client,
    ) -> anyhow::Result<()> {
        let text_dir = format!("./{}/text", self.user);
        let media_dir = format!("./{}/media", self.user);
        let mut builder = DirBuilder::new();
        builder.recursive(true).create(&text_dir).await?;
        builder.recursive(true).create(&media_dir).await?;

        // Consumes the cached/swapped events, returns all non-redacted events.
        let available_events = cached_events
            .into_iter()
            .filter_map(|ev| match ev.kind {
                TimelineEventKind::PlainText { event: plain } => {
                    plain
                        .deserialize_as::<RoomMessageEvent>()
                        .ok()
                        .and_then(|plain| {
                            let MessageLikeEvent::Original(orig) = plain else {
                                return None
                            };
                            Some(orig)
                        })
                },
                TimelineEventKind::Decrypted(decrypted) => {
                    decrypted
                        .event
                        .deserialize_as::<RoomMessageEvent>()
                        .ok()
                        .and_then(|de| {
                            let MessageLikeEvent::Original(orig) = de else {
                                return None
                            };
                            Some(orig)
                        })
                },
                _ => None,
            })
            .collect::<Vec<OriginalMessageLikeEvent<RoomMessageEventContent>>>();

        let stream = tokio_stream::iter(available_events)
            .for_each(|ev| async move {
                // I don't like this.
                let mut text_path = FilePath::new(format!("{}/text", self.user).into());
                let mut media_path = FilePath::new(format!("{}/media", self.user).into());

                match ev.content.msgtype {
                    MessageType::Text(ref text) => {
                        text_path.set_filename("text-export.txt");
                        if let Err(e) = text.process_event(&ev, &text_path).await {
                            tracing::event!(Level::ERROR, "MessageType::Text | Event ID: {} | Err: {e}", &ev.event_id);
                        }
                    }
                    MessageType::File(ref file) => {
                        media_path.set_filename(file.filename());
                        if let Err(e) = file.process_event(&client.clone(), &media_path).await {
                            tracing::event!(Level::ERROR, "MessageType::File | Event ID: {} | Err: {e}", &ev.event_id);
                        }
                    }
                    MessageType::Image(ref image) => {
                        media_path.set_filename(image.filename());
                        if let Err(e) = image.process_event(&client.clone(), &media_path).await {
                            tracing::event!(Level::ERROR, "MessageType::Image | Event ID: {} | Err: {e}", &ev.event_id);
                        }
                    }
                    MessageType::Video(ref video) => {
                        media_path.set_filename(video.filename());
                        if let Err(e) = video.process_event(&client.clone(), &media_path).await {
                            tracing::event!(Level::ERROR, "MessageType::Video | Event ID: {} | Err: {e}", &ev.event_id);
                        }
                    }
                    MessageType::Audio(ref audio) => {
                        media_path.set_filename(audio.filename());
                        if let Err(e) = audio.process_event(&client.clone(), &media_path).await {
                            tracing::event!(Level::ERROR, "MessageType::Audio | Event ID: {} | Err: {e}", &ev.event_id);
                        }
                    }
                    MessageType::Emote(_)
                    | MessageType::Notice(_)
                    | MessageType::ServerNotice(_)
                    | MessageType::Location(_)
                    | MessageType::VerificationRequest(_)
                    | _ => (),
                }
            });

        stream.await;

        anyhow::Ok(())
    }
}
