use std::io::stdout;

use matrix_sdk::ruma::OwnedRoomId;
use promkit::crossterm::{ExecutableCommand, cursor, style::Stylize};
use serde::{Deserialize, Serialize};
use tokio::{fs::File, io::AsyncWriteExt, sync::mpsc::Receiver};

/// The "interval" for caching data.
///
/// This is not represented as a time interval, but a denominator for the current amount of chunks.
/// For example, see [`utils::export::fetch_chunks`]:
///
/// ```
/// if curr_chunk.is_multiple_of(CACHE_INTERVAL) {
///     cache_tx.send(token.clone()).await?;
/// }
/// ```
///
/// Since 1 chunk = 100 messages, this will run the cache every 10.000 messages.
pub const CACHE_INTERVAL: u64 = 100;

/// File name for the cache.
const CACHE_FILE: &'static str = "met-cache.json";

/// Main struct for cache data
///
/// On larger exports, this cache gets created, and written to a file.
/// The token here is of the last fetched message chunk.
///
/// When the program runs again, this is imported (deserialized), and the export function
/// continues from `last_token`, instead of starting over.
#[derive(Clone, Serialize, Deserialize)]
pub struct RoomExportCache {
    /// Collection of room IDs
    room_id: Option<OwnedRoomId>,
    /// Collection of pagination tokens
    last_token: Option<String>,
}

/// The final export cache. This is a collection of [`RoomExportCache`]s that can be exported to a
/// file.
#[derive(Clone, Serialize, Deserialize)]
struct ExportCache {
    inner: Vec<RoomExportCache>,
}

impl RoomExportCache {
    /// Create a new, empty export cache.
    pub fn new() -> Self {
        Self {
            room_id: None,
            last_token: None,
        }
    }

    pub fn last_token(&self) -> Option<&String> {
        self.last_token.as_ref()
    }

    /// Add room data to the cache.
    pub fn add_room_data(&mut self, room_id: OwnedRoomId, last_token: Option<String>) {
        self.room_id.replace(room_id);
        if let Some(token) = last_token {
            self.last_token.replace(token);
        }
    }

    /// Update the token of this room through a channel
    ///
    /// This should be used inside a background task.
    pub async fn update_token(&mut self, mut new_token: Receiver<String>) -> anyhow::Result<()> {
        while let Some(new_token) = new_token.recv().await {
            stdout().execute(cursor::MoveDown(2))?;
            println!("(cache) Saving token {}", new_token);

            stdout().execute(cursor::RestorePosition)?;

            self.last_token.replace(new_token);
            self.write_cache().await?;
        }

        anyhow::Ok(())
    }

    /// Writes [`self`] to the cache file as json.
    async fn write_cache(&self) -> anyhow::Result<()> {
        let mut cache_file = File::create(CACHE_FILE).await?;

        let serialized = serde_json::to_string(&self).unwrap();

        cache_file.write(serialized.as_bytes()).await?;
        cache_file.flush().await?;

        anyhow::Ok(())
    }

    /// Import the cache from a file.
    ///
    /// This is synchronous, as it is used before starting any downloads.
    ///
    /// If the file is not found OR is invalid, a new cache is made.
    pub fn import_cache() -> Self {
        let file = std::fs::read_to_string(CACHE_FILE);

        if let Ok(file) = file {
            println!("{}", "Cache file found, recovering...".yellow());

            match serde_json::from_str::<Self>(&file) {
                Ok(cache) => {
                    println!("{}", "Cache file recovered.".green());
                    return cache;
                }
                Err(e) => {
                    println!(
                        "{}\n{} {}\n{}",
                        "Cache file couldn't be recovered.".red(),
                        "Reason:".white().bold(),
                        e,
                        "Making a new cache in memory instead.".white()
                    );
                    return Self::default();
                }
            }
        } else {
            println!("{}", "Cache file not found, making a new one.".white());
            return Self::default();
        }
    }
}

impl Default for RoomExportCache {
    fn default() -> Self {
        Self::new()
    }
}
