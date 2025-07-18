use std::io::stdout;

use matrix_sdk::ruma;
use matrix_sdk::{Room, deserialized_responses::TimelineEvent, room::MessagesOptions};

use tokio::sync::mpsc;

use promkit::crossterm::{ExecutableCommand, cursor, style::Stylize};

use crate::utils::cache::{
    output_cache::{self},
    user_cache,
};

/// Fetch message chunks and send to a receiver
async fn fetch_chunks(
    room: Room,
    mut user_cache: user_cache::RoomExportCache,
    mut out_cache: output_cache::FileCache,
) -> anyhow::Result<()> {
    let mut options = MessagesOptions::backward();
    // Load a cached token, if one exists
    if let Some(token) = user_cache.last_token() {
        println!(
            "{}",
            "Last message token found in cache, resuming from it instead."
                .green()
                .italic()
        );
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

    let (msg_tx, msg_rx) = mpsc::channel::<Vec<TimelineEvent>>(100);
    let (user_cache_tx, user_cache_rx) = mpsc::channel::<String>(1);
    let (write_tx, write_rx) = mpsc::channel::<bool>(1);

    // Background tasks for caching a token and the output itself.
    tokio::spawn(async move { user_cache.update_token(user_cache_rx).await });
    tokio::spawn(async move { out_cache.update_messages(msg_rx, write_rx).await });

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
        if curr_chunk.is_multiple_of(user_cache::CACHE_INTERVAL) {
            user_cache_tx.send(token.clone()).await?;
            write_tx.send(true).await?;
        }

        options = MessagesOptions::backward().from(&*token);
    }

    println!("{}: {}", name, "Fetched all messages".green().italic());
    anyhow::Ok(())
}

/// Export messages to a file
pub async fn export_room(room: Room) -> anyhow::Result<()> {
    let name = room.display_name().await?.to_string();

    let mut user_cache = user_cache::RoomExportCache::import_cache();
    let out_cache = output_cache::FileCache::new(name.clone());

    user_cache.add_room_data(room.room_id().to_owned(), None);

    let fetch_handle = tokio::spawn(async move {
        if let Err(e) = fetch_chunks(room, user_cache, out_cache).await {
            eprintln!("Couldn't fetch events: {}", e);
            return;
        }
    });

    fetch_handle
        .await
        .map_err(|e| anyhow::anyhow!("Fetch task failed: {}", e))?;

    println!("{}: {}", name.bold(), "Export complete".bold().italic());
    anyhow::Ok(())
}
