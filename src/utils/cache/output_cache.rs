use std::{io::stdout, path::PathBuf};

use matrix_sdk::{
    Client,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    ruma::events::{
        MessageLikeEvent, OriginalMessageLikeEvent,
        room::message::{MessageType, RoomMessageEvent, RoomMessageEventContent},
    },
    stream::StreamExt,
};
use promkit::core::crossterm::{
    ExecutableCommand, cursor,
    terminal::{Clear, ClearType},
};
use tokio::{
    fs::{DirBuilder, OpenOptions},
    sync::mpsc::{self, Receiver},
};

use crate::utils::traits::{ProcessableMediaEvent, ProcessableTextEvent, TextBuffer};

#[derive(Clone)]
pub struct FileCache {
    user: String,
    dirs: WriteDirs,
}

#[derive(Clone, Debug)]
pub struct WriteDirs {
    //user_dir: PathBuf,
    //text_dir: PathBuf,
    media_dir: PathBuf,
    text_file: PathBuf,
}

impl WriteDirs {
    /// Create dirs and return their paths
    async fn setup_files(user: &String) -> anyhow::Result<Self> {
        let mut dir_builder = DirBuilder::new();
        let user_dir = PathBuf::from(&user);
        let (text_dir, media_dir) = (user_dir.join("text"), user_dir.join("media"));
        let text_file = text_dir
            .join(format!("{} - messages", user))
            .with_extension("txt");

        dir_builder.recursive(true).create(&text_dir).await?;
        dir_builder.create(&media_dir).await?;
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&text_file)
            .await?;

        let media_types = ["Files", "Images", "Video", "Audio"];
        for media in media_types {
            dir_builder.create(&media_dir.join(media)).await?;
        }

        let dirs = Self {
            media_dir,
            text_file,
        };

        anyhow::Ok(dirs)
    }

    pub fn media_dir(&self) -> &PathBuf {
        &self.media_dir
    }
}

impl FileCache {
    pub async fn new(user: String) -> anyhow::Result<Self> {
        let dirs = WriteDirs::setup_files(&user).await?;
        let cache = Self { user, dirs };
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
        let mut media_buffer = Vec::new();

        while let Some(list) = msg_rx.recv().await {
            let (text, mut media) = Self::split_event_types(list);

            // It's okay to directly process text here.
            let text_buffer = &text_buffer.clone();
            tokio_stream::iter(text)
                .for_each_concurrent(None, |text_ev| async move {
                    if let Err(e) = text_ev.send_to_process(&text_buffer).await {
                        tracing::error!(
                            "Message type: {} | Err: {e} | Event ID: {} | Event timestamp: {:?}",
                            text_ev.content.msgtype(),
                            text_ev.event_id,
                            text_ev.origin_server_ts
                        );
                    }
                })
                .await;
            media_buffer.append(&mut media);

            if let Ok(_) = write_rx.try_recv() {
                text_buffer.write().ok();
            }
        }

        stdout().execute(Clear(ClearType::All))?;
        stdout().execute(cursor::MoveTo(0, 1))?;
        println!(
            "Completed text export, downloading {} files (see logs)",
            &media_buffer.len()
        );
        media_buffer.send_to_process(&client, &paths.clone()).await;

        anyhow::Ok(())
    }

    /// This filters out redacted events, and returns two vectors with original events.
    ///
    /// The first vector is for text events, and the latter for all other events.
    // TODO: Check whether this can be improved (return types).
    fn split_event_types(
        list: Vec<TimelineEvent>,
    ) -> (
        Vec<OriginalMessageLikeEvent<RoomMessageEventContent>>,
        Vec<OriginalMessageLikeEvent<RoomMessageEventContent>>,
    ) {
        let (text, media): (Vec<_>, Vec<_>) = list
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
            .partition(|ev| match ev.content.msgtype {
                // Text is handled separately from media.
                MessageType::Text(_) => return true,
                _ => return false,
            });

        return (text, media);
    }
}
