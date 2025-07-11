use std::io::stdout;

use matrix_sdk::{
    Room,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    room::MessagesOptions,
};
use matrix_sdk::{
    ruma,
    ruma::events::{MessageLikeEvent, room::message::RoomMessageEvent},
};

use tokio::{fs::File, io::AsyncWriteExt, sync::mpsc};
use tokio_util::sync::CancellationToken;

use promkit::crossterm::{ExecutableCommand, cursor, style::Stylize};

/// Fetch message chunks and send to a receiver
async fn fetch_chunks(
    room: Room,
    tx: mpsc::Sender<Vec<TimelineEvent>>,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    let mut options = MessagesOptions::backward();
    let mut total = 0;
    // make name pwetty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

    loop {
        if cancellation_token.is_cancelled() {
            println!("{}: {}", name, "Fetch cancelled".yellow());
            break;
        }

        stdout().execute(cursor::SavePosition)?;
        // 100 is the max (or so it seems)
        options.limit = ruma::UInt::from(100u8);

        let page = room.messages(options).await?;
        let chunk = page.chunk;

        total += chunk.len();
        println!(
            "{}: Fetched {} messages (total: {})",
            name,
            chunk.len(),
            total
        );
        stdout().execute(cursor::RestorePosition)?;

        if let Err(_) = tx.send(chunk).await {
            break;
        }

        let Some(token) = page.end else {
            break;
        };

        options = MessagesOptions::backward().from(&*token);
    }

    if !cancellation_token.is_cancelled() {
        println!("{}: {}", name, "Fetched all messages".green().italic());
    }
    anyhow::Ok(())
}

/// Export messages to a file
pub async fn export_room(room: Room, cancellation_token: CancellationToken) -> anyhow::Result<()> {
    let name = room.display_name().await?.to_string();
    let mut file = File::create(format!("{}-export.txt", name)).await?;

    // channel to download messages and write to file
    // before, it fetched everything to memory *then* wrote, not good.
    let (tx, mut rx) = mpsc::channel::<Vec<TimelineEvent>>(100);

    let fetch_token = cancellation_token.clone();
    let fetch_handle = tokio::spawn(async move {
        if let Err(e) = fetch_chunks(room, tx, fetch_token).await {
            eprintln!("Couldn't fetch events: {}", e);
            return;
        }
    });

    loop {
        tokio::select! {
            chunk = rx.recv() => {
                let Some(chunk) = chunk else {
                    break;
                };

                // TODO: Issue #6 (handle other types)
                // this should be moved to its whole own thing
                for message in chunk {
                    if let TimelineEventKind::Decrypted(decrypted) = &message.kind {
                        if let Ok(MessageLikeEvent::Original(original)) =
                            decrypted.event.deserialize_as::<RoomMessageEvent>()
                        {
                            let line = format!(
                                "{:?} — {}: {}\n",
                                original.origin_server_ts,
                                original.sender,
                                original.content.body()
                            );

                            file.write_all(line.as_bytes()).await?;
                        }
                    }
                }
                file.flush().await?;
            }
            _ = cancellation_token.cancelled() => {
                println!("{}: {}", name.clone().bold(), "Export cancelled, stopping".yellow());
                break;
            }
        }
    }

    fetch_handle
        .await
        .map_err(|e| anyhow::anyhow!("Fetch task failed: {}", e))?;

    if !cancellation_token.is_cancelled() {
        println!("{}: {}", name.bold(), "Export complete".bold().italic());
    } else {
        println!("{}: {}", name.bold(), "Export stopped".yellow());
    }

    // extra io check
    file.flush().await?;
    anyhow::Ok(())
}
