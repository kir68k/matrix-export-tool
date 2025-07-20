use std::io::stdout;

use matrix_sdk::{
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    ruma::events::{MessageLikeEvent, room::message::RoomMessageEvent},
};
use promkit::crossterm::{ExecutableCommand, cursor};
use tokio::{fs::OpenOptions, io::AsyncWriteExt, sync::mpsc::Receiver};

//#[derive(Clone)]
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
    ) -> anyhow::Result<()> {
        let mut buf_events = 0;

        while let Some(mut new_events) = msg_rx.recv().await {
            buf_events += &new_events.len();
            stdout().execute(cursor::MoveDown(3))?;
            println!("(cache) Caching {} messages", buf_events);
            stdout().execute(cursor::RestorePosition)?;

            self.messages.append(&mut new_events);

            if let Ok(_) = write_rx.try_recv() {
                buf_events = 0;
                self.write_to_file().await?;
            }
        }

        anyhow::Ok(())
    }

    async fn write_to_file(&mut self) -> anyhow::Result<()> {
        let mut out_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(format!("{}-export.txt", self.user))
            .await?;

        let mut messages = Vec::new();
        std::mem::swap(&mut messages, &mut self.messages);

        // At first I thought this is gonna be bad and slow
        // turns out it's fine.
        // I guess that's coz "With the encryption feature, messages are decrypted if possible" :D
        let string: String = messages
            .into_iter()
            .map(|ev| {
                if let TimelineEventKind::Decrypted(de) = &ev.kind
                    && let Ok(MessageLikeEvent::Original(orig)) =
                        de.event.deserialize_as::<RoomMessageEvent>()
                {
                    format!(
                        "{:?} — {}: {}\n",
                        orig.origin_server_ts,
                        orig.sender,
                        orig.content.body()
                    )
                } else {
                    format!("Incorrect type.\n")
                }
            })
            .collect();

        out_file.write(string.as_bytes()).await?;
        out_file.flush().await?;

        anyhow::Ok(())
    }
}
