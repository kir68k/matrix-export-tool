use std::io::stdout;

use matrix_sdk::ruma::UInt;
use matrix_sdk::{Room, deserialized_responses::TimelineEvent, room::MessagesOptions};

use tokio::sync::mpsc;

use promkit::crossterm::{ExecutableCommand, cursor, style::Stylize};

use crate::utils::cache::{
    CACHE_INTERVAL,
    output_cache::{self},
    user_cache::{CACHE_DONE, ExportCache, RoomExportCache},
};

/// Convenience function, returns either a clone of the cached room, or a new one.
/// If it's a new one, it gets added to the cache.
fn get_room(cache: &ExportCache, room: &Room) -> RoomExportCache {
    let room_id = room.room_id().to_owned();
    let name = room.cached_display_name().unwrap().to_string();

    if let Some(room) = cache
        .get_inner()
        .unwrap()
        .rooms
        .iter()
        .find(|cached| cached.room_id().unwrap() == &room_id)
    {
        return room.clone();
    } else {
        let mut room = RoomExportCache::default();
        room.add_room_data(room_id, name, None).add_to_global(cache);

        return room;
    }
}

/// Fetch message chunks and send to a receiver
async fn fetch_chunks(
    room: Room,
    cache: &ExportCache,
    mut out_cache: output_cache::FileCache,
) -> anyhow::Result<()> {
    let mut options = MessagesOptions::backward();

    let room_cache = get_room(cache, &room);

    // Load a cached token, if one exists
    if let Some(token) = room_cache.last_token() {
        if token == CACHE_DONE {
            return Err(anyhow::anyhow!("This export is marked as completed."));
        }

        println!(
            "{}",
            "Last message token found in cache, resuming from it instead."
                .green()
                .italic()
        );
        options = options.from(token.as_str());
    }

    // make name pwetty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

    let (msg_tx, msg_rx) = mpsc::channel::<Vec<TimelineEvent>>(100);
    let (room_cache_tx, room_cache_rx) = mpsc::channel::<String>(1);
    let (write_tx, write_rx) = mpsc::channel::<bool>(1);

    // Background tasks for caching a token and the output itself.
    let cache = cache.clone();
    #[rustfmt::skip]
    tokio::spawn(async move {
        room_cache.update_token(room_cache_rx, &cache).await
    });
    #[rustfmt::skip]
    tokio::spawn(async move {
        out_cache.update_messages(msg_rx, write_rx).await
    });

    // This is used for caching (and the other to let user know progress).
    let mut curr_chunk: u64 = 0;
    let mut total = 0;
    loop {
        stdout().execute(cursor::SavePosition)?;
        // 100 is the max (or so it seems)
        options.limit = UInt::from(100u8);

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

        if let Some(token) = page.end {
            if curr_chunk.is_multiple_of(CACHE_INTERVAL) {
                room_cache_tx.send(token.clone()).await?;
                write_tx.send(true).await?;
            }

            options = MessagesOptions::backward().from(&*token);
            continue;
        } else {
            // on shorter exports below interval (or done ones), there's no point setting a real token.
            room_cache_tx.send(CACHE_DONE.to_string()).await?;
            write_tx.send(true).await?;
            break;
        }
    }

    println!("{}: {}", name, "Fetched all messages".green().italic());
    anyhow::Ok(())
}

/// Export messages to a file
pub async fn export_room(room: Room, cache: ExportCache) -> anyhow::Result<()> {
    let name = room.display_name().await?.to_string();

    let out_cache = output_cache::FileCache::new(name.clone());

    let fetch_handle = tokio::spawn(async move {
        if let Err(e) = fetch_chunks(room, &cache, out_cache).await {
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
