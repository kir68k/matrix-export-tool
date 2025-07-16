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

use promkit::crossterm::{ExecutableCommand, cursor, style::Stylize};

use crate::utils::cache::{self, RoomExportCache, CACHE_INTERVAL};

/// Fetch message chunks and send to a receiver
async fn fetch_chunks(
    room: Room,
    msg_tx: mpsc::Sender<Vec<TimelineEvent>>,
    mut cache: RoomExportCache
) -> anyhow::Result<()> {
    let mut options = MessagesOptions::backward();
    // Load a cached token, if one exists
    if let Some(token) = cache.last_token() {
        println!("{}", "Last message token found in cache, resuming from it instead.".green().italic());
        options = options.from(token.as_str());
    }

    let mut total = 0;
    // make name pwetty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

    let (cache_tx, cache_rx) = mpsc::channel::<String>(1);
    cache.add_room_data(room.room_id().to_owned(), None);

    // Background task, waits time, receives the last token from the main loop
    // and sends it to the cache file.
    tokio::spawn(async move {
        cache.update_token(cache_rx).await
    });

    // This is used for caching.
    let mut curr_chunk: u64 = 0;
    loop {
        stdout().execute(cursor::SavePosition)?;
        // 100 is the max (or so it seems)
        options.limit = ruma::UInt::from(100u8);

        let page = room.messages(options).await?;
        let chunk = page.chunk;

        total += chunk.len();
        curr_chunk += 1;
        println!(
            "{}: Fetched {} messages (total: {})",
            name,
            chunk.len(),
            total
        );
        stdout().execute(cursor::RestorePosition)?;

        if let Err(_) = msg_tx.send(chunk).await {
            break;
        }

        let Some(token) = page.end else {
            break;
        };

        // Run cache, currently every 10.000 messages.
        if curr_chunk.is_multiple_of(CACHE_INTERVAL) {
            cache_tx.send(token.clone()).await?;
        }

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
    let mut cache = RoomExportCache::import_cache()?;

    let fetch_handle = tokio::spawn(async move {
        if let Err(e) = fetch_chunks(room, tx, cache).await {
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
