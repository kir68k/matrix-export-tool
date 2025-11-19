use std::{io::stdout, path::PathBuf};

use serde_json::Value;

use matrix_sdk::{
    Client,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    ruma::events::{
        MessageLikeEvent,
        room::message::{MessageType, OriginalRoomMessageEvent, RoomMessageEvent},
    },
    stream::StreamExt,
};
use promkit::core::crossterm::{
    ExecutableCommand, cursor,
    terminal::{Clear, ClearType},
};
use tokio::{
    fs::{DirBuilder, OpenOptions},
    io::AsyncWriteExt,
    sync::mpsc::Receiver,
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
    /// For serialized output of all events
    serialized_file: PathBuf,
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

        let serialized_dir = user_dir.join("events");
        let serialized_file = serialized_dir
            .join(format!("{} - Events", user))
            .with_extension("ron");

        // create dirs
        dir_builder.recursive(true).create(&text_dir).await?;
        dir_builder.create(&media_dir).await?;
        dir_builder.create(&serialized_dir).await?;
        // create files for formatted output & event list
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&text_file)
            .await?;
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&serialized_file)
            .await?;

        let media_types = ["Files", "Images", "Video", "Audio"];
        for media in media_types {
            dir_builder.create(&media_dir.join(media)).await?;
        }

        let dirs = Self {
            media_dir,
            text_file,
            serialized_file,
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

        let mut serialized_events = Vec::new();
        let mut last_serialized_len: usize = 0;

        while let Some(list) = msg_rx.recv().await {
            let (mut serialized, deserialized) = Self::split_serialized_deserialized(list);
            serialized_events.append(&mut serialized);

            let (text, mut media) = Self::split_event_types(deserialized);

            // It's okay to directly process text here.
            let text_buffer = &text_buffer.clone();
            tokio_stream::iter(text)
                .for_each_concurrent(256, |text_ev| async move {
                    if let Err(e) = text_ev.send_to_process(text_buffer).await {
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

            if write_rx.try_recv().is_ok() {
                text_buffer.write().ok();
                // Write raw event data to file
                // The point is to have an export of all decrypted/plain events,
                // which can be imported later for whatever purpose.
                //
                // Rationale: I have to nuke my homeserver but want to keep event data.
                // This will become more useful in the future.
                if serialized_events.len() != last_serialized_len {
                    // mostly for debugging.
                    stdout().execute(Clear(ClearType::All))?;
                    stdout().execute(cursor::MoveTo(0, 1))?;
                    println!(
                        "Writing {} serialized events to file...",
                        serialized_events.len()
                    );

                    let serialized = ron::ser::to_string_pretty(
                        &serialized_events,
                        ron::ser::PrettyConfig::default(),
                    )
                    .unwrap();

                    // The file is created by WriteDirs::setup_files(), and overridden on each signal.
                    let mut serialized_file = OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&paths.serialized_file)
                        .await?;

                    serialized_file.write_all(serialized.as_bytes()).await?;
                    serialized_file.flush().await?;

                    last_serialized_len = serialized_events.len();
                }
            }
        }

        serialized_events.clear();
        serialized_events.shrink_to_fit();

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
    fn split_event_types(
        events: Vec<OriginalRoomMessageEvent>,
    ) -> (Vec<OriginalRoomMessageEvent>, Vec<OriginalRoomMessageEvent>) {
        let (text, media): (Vec<_>, Vec<_>) =
            events.into_iter().partition(|ev| match ev.content.msgtype {
                // Text is handled separately from media.
                MessageType::Text(_) => true,
                _ => false,
            });

        (text, media)
    }


    /// Takes a list of received events, filters redacted/unable to decrypt, and returns two vecs.
    /// One vec is for JSON of the events, the other for their deserialized form.
    fn split_serialized_deserialized(
        list: Vec<TimelineEvent>,
    ) -> (Vec<Value>, Vec<OriginalRoomMessageEvent>) {
        let mut serialized = Vec::new();
        let mut deserialized = Vec::new();

        for ev in list {
            match ev.kind {
                TimelineEventKind::PlainText { event: plain } => {
                    if let Ok(plain_ev) = plain.deserialize_as_unchecked::<RoomMessageEvent>()
                        && let MessageLikeEvent::Original(orig) = plain_ev
                    {
                        if let Ok(val) = serde_json::to_value(&plain) {
                            serialized.push(val);
                        }
                        deserialized.push(orig);
                    }
                }
                TimelineEventKind::Decrypted(decrypted) => {
                    if let Ok(de) = decrypted
                        .event
                        .deserialize_as_unchecked::<RoomMessageEvent>()
                        && let MessageLikeEvent::Original(orig) = de
                    {
                        if let Ok(val) = serde_json::to_value(&decrypted.event) {
                            serialized.push(val);
                        }
                        deserialized.push(orig);
                    }
                }
                _ => {}
            }
        }

        (serialized, deserialized)
    }
}
