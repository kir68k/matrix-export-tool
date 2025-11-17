use std::{
    io::{Write, stdout},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use matrix_sdk::ruma::OwnedRoomId;
use promkit::core::crossterm::{ExecutableCommand, cursor, style::Stylize};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Receiver;

/// File name for the cache.
const CACHE_FILE: &str = "met-cache.json";

/// A dummy token. If this is set, the export is skipped.
// FIXME: This should be its own field in the cache. This works for now,
// but later mby keep the last token, and turn this field to `true`.
pub const CACHE_DONE: &str = "export_completed";

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

    /// Room display name
    display_name: Option<String>,
}

/// The final export cache. This is a collection of [`RoomExportCache`]s that can be exported to a
/// file.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExportCache {
    inner: Arc<Mutex<ExportCacheInner>>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ExportCacheInner {
    pub rooms: Vec<RoomExportCache>,
}

impl ExportCacheInner {
    fn new() -> Self {
        Self { rooms: Vec::new() }
    }
}

impl ExportCache {
    pub fn new() -> Self {
        let cache = Mutex::new(ExportCacheInner::new());

        Self {
            inner: Arc::new(cache),
        }
    }

    fn add_room(&self, room: RoomExportCache) {
        self.inner.lock().unwrap().rooms.push(room);
    }

    /// Replace an older token of `room_id` in the export cache with a new one
    pub fn update_room_token(&self, room_id: &OwnedRoomId, token: String) -> anyhow::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .rooms
            .iter_mut()
            .find(|room| room.room_id().unwrap() == room_id)
            .ok_or(anyhow::anyhow!("No room found."))
            .and_then(|room_cache| room_cache.set_token(token))
    }

    pub fn get_inner(
        &self,
    ) -> anyhow::Result<
        MutexGuard<'_, ExportCacheInner>,
        PoisonError<MutexGuard<'_, ExportCacheInner>>,
    > {
        self.inner.lock()
    }

    /// Writes [`self`] to the cache file as json.
    pub fn write_cache(&self) -> anyhow::Result<()> {
        let mut cache_file = std::fs::File::create(CACHE_FILE)?;
        let serialized = serde_json::to_string(self).unwrap();

        cache_file.write_all(serialized.as_bytes())?;

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
                    cache
                }
                Err(e) => {
                    println!(
                        "{}\n{} {}\n{}",
                        "Cache file couldn't be recovered.".red(),
                        "Reason:".white().bold(),
                        e,
                        "Making a new cache in memory instead.".white()
                    );
                    Self::new()
                }
            }
        } else {
            println!("{}", "Cache file not found, making a new one.".white());
            Self::new()
        }
    }
}

impl RoomExportCache {
    /// Create a new, empty export cache.
    pub fn new() -> Self {
        Self {
            room_id: None,
            last_token: None,
            display_name: None,
        }
    }

    pub fn last_token(&self) -> Option<&String> {
        self.last_token.as_ref()
    }

    pub fn room_id(&self) -> Option<&OwnedRoomId> {
        self.room_id.as_ref()
    }

    pub fn display_name(&self) -> Option<&String> {
        self.display_name.as_ref()
    }

    pub fn set_token(&mut self, token: String) -> anyhow::Result<()> {
        self.last_token.replace(token);
        anyhow::Ok(())
    }

    pub fn add_to_global(&self, global: &ExportCache) {
        global.add_room(self.to_owned());
    }

    /// Add room data to the cache.
    pub fn add_room_data(
        &mut self,
        room_id: OwnedRoomId,
        display_name: String,
        last_token: Option<String>,
    ) -> &Self {
        self.room_id.replace(room_id);
        self.display_name.replace(display_name);
        if let Some(token) = last_token {
            self.last_token.replace(token);
        }

        &*self
    }

    /// Update the token of this room through a channel
    ///
    /// This should be used inside a background task.
    pub async fn update_token(
        &self,
        mut new_token: Receiver<String>,
        cache: &ExportCache,
    ) -> anyhow::Result<()> {
        while let Some(token) = new_token.recv().await {
            stdout().execute(cursor::MoveDown(2))?;
            println!(
                "(cache) {}: Saving token {}",
                self.display_name().unwrap(),
                token
            );

            stdout().execute(cursor::RestorePosition)?;

            cache.update_room_token(self.room_id().unwrap(), token)?;
            cache.write_cache()?;
        }

        anyhow::Ok(())
    }
}

impl Default for RoomExportCache {
    fn default() -> Self {
        Self::new()
    }
}
