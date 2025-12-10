use std::{
    io::stdout,
    path::{Path, PathBuf},
};

use serde_json::Value as JsonValue;

use matrix_sdk::{
    Client,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
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

use crate::utils::media::{
    DownloadableMedia, MediaKind, ProcessableEvent, ProcessableMediaEvent,
    text::{TextBuffer, process_text_event},
};

#[derive(Clone)]
pub struct FileCache {
    dirs: WriteDirs,
}

#[derive(Clone, Debug)]
pub struct WriteDirs {
    user: String,
    media_dir: PathBuf,
    text_file: PathBuf,
    /// For raw JSON of all events.
    serialized_dir: PathBuf,

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
            .with_extension("json");

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

        let media_types = MediaKind::subdirs();
        for media in media_types {
            dir_builder
                .recursive(true)
                .create(&media_dir.join(media))
                .await?;
        }

        let dirs = Self {
            media_dir,
            text_file,
            serialized_dir,
            user: user.to_owned(),
        };

        anyhow::Ok(dirs)
    }

    /// Reference to the media directory path
    pub fn media_dir(&self) -> &PathBuf {
        &self.media_dir
    }

    /// File for the serialized events
    pub fn serialized_file(&self) -> PathBuf {
        self.serialized_dir
            .join(format!("{} - Events", self.user))
            .with_extension("json")
    }

    /// File for events which couldn't be decrypted
    pub fn utd_file(&self) -> PathBuf {
        self.serialized_dir
            .join(format!("{} - Unable to decrypt", self.user))
            .with_extension("json")
    }
}

impl FileCache {
    pub async fn new(user: String) -> anyhow::Result<Self> {
        let dirs = WriteDirs::setup_files(&user).await?;
        let cache = Self { dirs };
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
        let mut media_buffer: Vec<DownloadableMedia> = Vec::new();

        let (mut serialized_events, mut utd_events) = (Vec::new(), Vec::new());
        let (mut serialized_len, mut utd_len) = (0, 0);

        while let Some(list) = msg_rx.recv().await {
            let (mut serialized, events, mut utd) = Self::split_event_kinds(list);
            serialized_events.append(&mut serialized);
            utd_events.append(&mut utd);

            // Process the arrived evs
            for event in events {
                match event {
                    ProcessableEvent::Text { body, metadata } => {
                        process_text_event(&body, &metadata, &text_buffer);
                    }
                    ProcessableEvent::Media(media) => {
                        media_buffer.push(media);
                    }
                }
            }

            if write_rx.try_recv().is_ok() {
                text_buffer.write().ok();
                // Write raw event data to file
                // The point is to have an export of all decrypted/plain events,
                // which can be imported later for whatever purpose.
                //
                // Rationale: I have to nuke my homeserver but want to keep event data.
                // This will become more useful in the future.
                if serialized_events.len() != serialized_len {
                    // mostly for debugging.
                    stdout().execute(Clear(ClearType::All))?;
                    stdout().execute(cursor::MoveTo(0, 1))?;
                    println!(
                        "Writing {} total serialized events to file...",
                        serialized_events.len()
                    );

                    Self::write_serialized(&serialized_events, &paths.serialized_file()).await?;

                    serialized_len = serialized_events.len();
                }

                if utd_events.len() != utd_len {
                    if let Err(e) = Self::write_serialized(&utd_events, &paths.utd_file()).await {
                        tracing::error!("Failed writing UTD events: {e}");
                    } else {
                        utd_len = utd_events.len();
                    }
                }
            }
        }

        // Unneeded after the loop exits.
        // todo: test more whether this is needed :p
        serialized_events.clear();
        serialized_events.shrink_to_fit();
        utd_events.clear();
        utd_events.shrink_to_fit();

        stdout().execute(Clear(ClearType::All))?;
        stdout().execute(cursor::MoveTo(0, 1))?;
        println!(
            "Completed text export, downloading {} files (see logs)",
            &media_buffer.len()
        );
        media_buffer.send_to_process(&client, &paths.clone()).await;

        anyhow::Ok(())
    }

    /// Takes a list of events, filters redacted, and returns three vecs:
    /// serialized (JSON), processable events, unable to decrypt (serialized JSON)
    fn split_event_kinds(
        list: Vec<TimelineEvent>,
    ) -> (Vec<JsonValue>, Vec<ProcessableEvent>, Vec<JsonValue>) {
        let mut serialized = Vec::new();
        let mut processable = Vec::new();
        let mut utd = Vec::new();

        for ev in list {
            match ev.kind {
                TimelineEventKind::PlainText { event: plain } => {
                    if let Ok(plain_ev) = plain.deserialize() {
                        if let Ok(val) = serde_json::to_value(&plain) {
                            serialized.push(val);
                        }

                        if let Some(proc_ev) = ProcessableEvent::try_from_sync(plain_ev) {
                            processable.push(proc_ev);
                        }
                    }
                }
                TimelineEventKind::Decrypted(decrypted) => {
                    if let Ok(decrypted_ev) = decrypted.event.deserialize() {
                        if let Ok(val) = serde_json::to_value(&decrypted.event) {
                            serialized.push(val);
                        }

                        if let Some(proc_ev) = ProcessableEvent::try_from_full(decrypted_ev) {
                            processable.push(proc_ev);
                        }
                    }
                }
                TimelineEventKind::UnableToDecrypt {
                    event: utd_raw,
                    utd_info: _,
                } => {
                    if let Ok(_) = utd_raw.deserialize()
                        && let Ok(val) = serde_json::to_value(&utd_raw)
                    {
                        utd.push(val);
                    }
                }
            }
        }

        (serialized, processable, utd)
    }

    /// Takes a list of events (or any [`JsonValue`]) with a full `path` and writes to it.
    async fn write_serialized(
        serialized_events: &Vec<JsonValue>,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let serialized = serde_json::to_string_pretty(serialized_events)?;

        let mut serialized_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await?;

        let write = serialized_file.write_all(serialized.as_bytes()).await;
        let flush = serialized_file.flush().await;
        match (write, flush) {
            (Ok(_), Ok(_)) => Ok(()),
            (Ok(_), Err(e)) => Err(anyhow::anyhow!("Error finishing serialized write: {e}")),
            (Err(e), Ok(_)) => Err(anyhow::anyhow!("Error writing serialized write: {e}")),
            (Err(we), Err(fe)) => Err(anyhow::anyhow!(
                "Errors writing to serialized file: {we}\n{fe}"
            )),
        }
    }
}
