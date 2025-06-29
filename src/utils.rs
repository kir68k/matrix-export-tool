use crate::cli::interface::UserInfo;
use anyhow::{Error, Ok, Result, bail};
use matrix_sdk::{
    Client,
    config::SyncSettings,
    ruma::{OwnedRoomId, UserId},
};
use promkit::preset::checkbox::Checkbox;

/// Log-in using a password and create a client
pub async fn login(user: &UserInfo) -> Result<Client> {
    let uid = UserId::parse(&user.userid)?;

    let client = Client::builder()
        .server_name(uid.server_name())
        .build()
        .await?;

    client
        .matrix_auth()
        .login_username(&uid, &user.password)
        .initial_device_display_name("matrix-export-tool")
        .await?;

    Ok(client)
}

// Basically a hack for prompt to show display name
// but use RoomId internally
// yes a custom prompt is better
#[derive(Clone)]
struct RoomDisplayInfo {
    display_name: String,
    room_id: OwnedRoomId,
}

impl std::fmt::Display for RoomDisplayInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name)
    }
}

/// Prompt the user with a list of every [`Room`] they've joined.
pub async fn select_room(client: &Client) -> Result<Vec<OwnedRoomId>, Error> {
    // Sync just in case.
    client.sync_once(SyncSettings::default()).await?;
    let rooms = client.joined_rooms();
    let mut room_names: Vec<RoomDisplayInfo> = Vec::new();

    // Kinda ugly but it works
    // This makes [`RoomDisplayInfo`] for each room
    for room in rooms {
        // Get the display name, default to ID if unavailable (so it's not empty)
        let display_name = room
            .display_name()
            .await
            .map(|name| name.to_string())
            .unwrap_or_else(|_| room.room_id().to_string() + "  (name unavailable)");

        room_names.push(RoomDisplayInfo {
            display_name,
            room_id: room.room_id().to_owned(),
        });
    }

    // The actual prompt
    // This returns Vec<String>, here display_name
    // TODO: Implement custom prompt instead of a preset
    // would get rid of weird stuff in this function...
    let selected = Checkbox::new(room_names.clone())
        .title("Select rooms to export")
        .checkbox_lines(10)
        .prompt()?
        .run()?;

    if selected.is_empty() {
        bail!("No rooms selected.");
    }

    let selected_ids: Vec<OwnedRoomId> = selected
        .into_iter()
        .filter_map(|selected| {
            room_names
                .iter()
                .find(|info| info.display_name == selected)
                .map(|info| info.room_id.clone())
        })
        .collect();

    Ok(selected_ids)
}
