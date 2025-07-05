use crate::cli::interface::UserInfo;
use anyhow::{Error, Result, bail};
use matrix_sdk::{
    Client, Room,
    config::SyncSettings,
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    room::MessagesOptions,
    ruma::{
        OwnedRoomId, UInt, UserId,
        events::{MessageLikeEvent, room::message::RoomMessageEvent},
    },
};
use promkit::crossterm::style::Stylize;
use promkit::preset::checkbox::Checkbox;
use tokio::{fs::File, io::AsyncWriteExt};

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

    anyhow::Ok(client)
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

    anyhow::Ok(selected_ids)
}

/// Fetch all available message chunks from server
pub async fn fetch_chunks(room: &Room) -> anyhow::Result<Vec<TimelineEvent>> {
    let mut options = MessagesOptions::backward();
    let mut result: Vec<TimelineEvent> = Vec::new();
    // make name pretty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

    loop {
        options.limit = UInt::from(100u8);

        let page = room.messages(options).await?;
        result.extend(page.chunk);
        println!("{}: Fetched {} messages", name, result.len());

        let Some(token) = page.end else {
            break;
        };

        // Reset options
        options = MessagesOptions::backward().from(&*token);
    }

    println!("{}: {}", name, "Fetched all messages".green().italic());
    // Reverse order (coz backward())
    result.reverse();

    anyhow::Ok(result)
}

/// Export messages to a file
pub async fn export_room(room: &Room) -> anyhow::Result<()> {
    let name = room.cached_display_name().unwrap().to_string();
    let mut file = File::create(format!("{}-export.txt", name)).await?;

    let messages = fetch_chunks(room).await?;

    for message in messages {
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
    println!("{}: {}", name.bold(), "Export complete".bold().italic());

    // extra io check
    file.flush().await?;
    anyhow::Ok(())
}
