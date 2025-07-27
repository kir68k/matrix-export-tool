use std::path::PathBuf;

use matrix_sdk::{
    Client,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    ruma::events::{
        MessageLikeEvent, OriginalMessageLikeEvent,
        room::message::{MessageType, RoomMessageEvent, RoomMessageEventContent},
    },
    stream::StreamExt,
};
use tokio::{fs::{DirBuilder, OpenOptions}, sync::mpsc::Receiver};

use crate::utils::traits::{TextBuffer, ValidEvent, ValidMediaEvent};

#[derive(Clone)]
pub struct FileCache {
    user: String,
    dirs: WriteDirs,
}

#[derive(Clone, Debug)]
struct WriteDirs {
    user_dir: PathBuf,
    text_dir: PathBuf,
    media_dir: PathBuf,
    text_file: PathBuf,
}

impl WriteDirs {
    /// Create dirs and return their paths
    async fn setup_files(user: &String) -> anyhow::Result<Self> {
        let mut dir_builder = DirBuilder::new();
        let user_dir = PathBuf::from(&user);
        let (text_dir, media_dir) = (user_dir.join("text"), user_dir.join("media"));
        let text_file = text_dir.join(format!("{} - messages", user)).with_extension("txt");

        dir_builder.recursive(true).create(&text_dir).await?;
        dir_builder.create(&media_dir).await?;
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&text_file).await?;

        let media_types = ["Files", "Images", "Video", "Audio"];
        for media in media_types {
            dir_builder.create(&media_dir.join(media)).await?;
        }

        let dirs = Self { user_dir, text_dir, media_dir, text_file };

        anyhow::Ok(dirs)
    }
}

impl FileCache {
    pub async fn new(user: String) -> anyhow::Result<Self> {
        let dirs = WriteDirs::setup_files(&user).await?;
        let cache = Self {
            user,
            dirs,
        };
        anyhow::Ok(cache)
    }

    pub async fn update_messages(
        &mut self,
        mut msg_rx: Receiver<Vec<TimelineEvent>>,
        mut write_rx: Receiver<bool>,
        client: Client,
    ) -> anyhow::Result<()> {
        let paths = self.dirs.clone();
        let text_buffer = TextBuffer::new(&paths.text_file);

        while let Some(list) = msg_rx.recv().await {
            Self::process_events(list, &paths.clone(), &text_buffer, &client.clone()).await;

            if let Ok(_) = write_rx.try_recv() {
                text_buffer.write().ok();
            }
        }

        anyhow::Ok(())
    }

    async fn process_events(list: Vec<TimelineEvent>, paths: &WriteDirs, buf: &TextBuffer, client: &Client) {
        let filtered_events = Self::filter_events(list).await;

        // Text is processed separately from media, fyi:
        // - Text: Sent to a buffer which waits for a write signal
        // - Media: Downloads/writes immediately.
        tokio_stream::iter(filtered_events).for_each_concurrent(None, |ev| async move {
            let res = match ev.content.msgtype {
                MessageType::Text(ref text) => {
                    text.process_event(&ev, &buf.clone()).await
                }
                MessageType::File(ref file) => {
                    file.process_event(&ev, &client.clone(), &paths.media_dir.join("Files")).await
                }
                MessageType::Image(ref image) => {
                    image.process_event(&ev, &client.clone(), &paths.media_dir.join("Images")).await
                }
                MessageType::Video(ref video) => {
                    video.process_event(&ev, &client.clone(), &paths.media_dir.join("Video")).await
                }
                MessageType::Audio(ref audio) => {
                    audio.process_event(&ev, &client.clone(), &paths.media_dir.join("Audio")).await
                }
                _ => anyhow::Ok(()),
            };

            if let Err(e) = res {
                tracing::error!(
                    "{} | Err: {e} | Event ID: {} | Event timestamp: {:?}",
                    ev.content.msgtype(),
                    ev.event_id,
                    ev.origin_server_ts
                );
            }
        }).await;
    }

    /// Filters out redacted events from a list.
    async fn filter_events (
        list: Vec<TimelineEvent>
    ) -> Vec<OriginalMessageLikeEvent<RoomMessageEventContent>> {
        list
            .into_iter()
            .filter_map(|ev| match ev.kind {
                TimelineEventKind::PlainText { event: plain } => plain
                    .deserialize_as::<RoomMessageEvent>()
                    .ok()
                    .and_then(|plain| {
                        let MessageLikeEvent::Original(orig) = plain else {
                            return None;
                        };
                        Some(orig)
                    }),
                TimelineEventKind::Decrypted(decrypted) => decrypted
                    .event
                    .deserialize_as::<RoomMessageEvent>()
                    .ok()
                    .and_then(|de| {
                        let MessageLikeEvent::Original(orig) = de else {
                            return None;
                        };
                        Some(orig)
                    }),
                _ => None,
            })
            .collect()
    }
}
