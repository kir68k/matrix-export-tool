use std::io::stdout;

use matrix_sdk::Client;
use matrix_sdk::ruma::UInt;
use matrix_sdk::{Room, deserialized_responses::TimelineEvent, room::MessagesOptions};

use tokio::sync::mpsc::{self, Sender};

use promkit::core::crossterm::{ExecutableCommand, cursor, style::Stylize};
use tokio::task::JoinSet;

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

/// Fetch event chunks, send a token on cache interval hits, send events continuously, and a write
/// signal. Write signals occur either when downloading finishes *or* the cache interval is hit.
async fn fetch_chunks(
    room: Room,
    mut options: MessagesOptions,
    room_cache_tx: Sender<String>,
    msg_tx: Sender<Vec<TimelineEvent>>,
    write_tx: Sender<bool>,
) -> anyhow::Result<()> {
    // make name pwetty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

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
pub async fn export_room(client: &Client, room: Room, cache: ExportCache) -> anyhow::Result<()> {
    let name = room.display_name().await?.to_string();

    let room_cache = get_room(&cache, &room);
    let mut out_cache = output_cache::FileCache::new(name.clone()).await?;

    let (msg_tx, msg_rx) = mpsc::channel::<Vec<TimelineEvent>>(100);
    let (room_cache_tx, room_cache_rx) = mpsc::channel::<String>(100);
    let (write_tx, write_rx) = mpsc::channel::<bool>(10);

    // Load a cached token, if one exists, or quit if marked done.
    let mut options = MessagesOptions::backward();
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

    // remember, both of these are Arc internally :3
    let output_client = client.clone();
    let global_cache = cache.clone();

    // All export tasks.
    let mut export_handle = JoinSet::new();

    // Download task
    export_handle.spawn(async move {
        if let Err(e) = fetch_chunks(room, options, room_cache_tx, msg_tx, write_tx).await {
            eprintln!("Couldn't fetch events: {e}");
            return;
        }
    });
    // Progress file/cache updates
    export_handle.spawn(async move {
        if let Err(e) = room_cache.update_token(room_cache_rx, &global_cache).await {
            eprintln!("Error updating export cache: {e}");
            return;
        }
    });
    // Event processing
    export_handle.spawn(async move {
        if let Err(e) = out_cache.update_messages(msg_rx, write_rx, output_client).await {
            eprintln!("Error processing or writing files: {e}");
            return;
        }
    });

    while let Some(res) = export_handle.join_next().await {
        match res {
            Ok(_) => (),
            Err(err) => eprintln!("Output task error: {err}"),
        }
    }

    println!("{}: {}", name.bold(), "Export complete".bold().italic());
    anyhow::Ok(())
}
