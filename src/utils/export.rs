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

use promkit::crossterm::{cursor, style::Stylize, ExecutableCommand};

/// Fetch message chunks and send to a receiver
async fn fetch_chunks(room: Room, tx: mpsc::Sender<Vec<TimelineEvent>>) -> anyhow::Result<()> {
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

    println!("{}: {}", name, "Fetched all messages".green().italic());
    anyhow::Ok(())
}

/// Export messages to a file
pub async fn export_room(room: Room) -> anyhow::Result<()> {
    let name = room.display_name().await?.to_string();
    let mut file = File::create(format!("{}-export.txt", name)).await?;

    // channel to download messages and write to file
    // before, it fetched everything to memory *then* wrote, not good.
    let (tx, mut rx) = mpsc::channel::<Vec<TimelineEvent>>(100);

    let fetch_handle = tokio::spawn(async move {
        if let Err(e) = fetch_chunks(room, tx).await {
            eprintln!("Couldn't fetch events: {}", e);
            return;
        }
    });

    // update: this still feels icky
    while let Some(chunk) = rx.recv().await {
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
    fetch_handle
        .await
        .map_err(|e| anyhow::anyhow!("Fetch task failed: {}", e))?;

    println!("{}: {}", name.bold(), "Export complete".bold().italic());

    // extra io check
    file.flush().await?;
    anyhow::Ok(())
}
