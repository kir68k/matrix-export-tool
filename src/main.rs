mod cli;
mod utils;

use cli::interface::UserInfo;

use matrix_sdk::deserialized_responses::{TimelineEvent, TimelineEventKind};
#[allow(unused_imports)]
use matrix_sdk::{
    Client, Room, RoomDisplayName, RoomState,
    config::SyncSettings,
    room::MessagesOptions,
    ruma::{
        OwnedRoomId, RoomId, UserId, events::SyncMessageLikeEvent,
        events::room::message::SyncRoomMessageEvent, room_id,
    },
};

/// Fetch all available messages from a room
async fn fetch_all_messages(
    client: &Client,
    room_id: &RoomId,
) -> Result<Vec<TimelineEvent>, anyhow::Error> {
    let room = client.get_room(room_id).unwrap();
    let mut fetched = Vec::new();
    let mut options = MessagesOptions::backward();

    // TODO: redo this aeugh (it *feels* odd...)
    // also coz forward() output is more intuitive
    loop {
        let messages = room.messages(options).await?;

        if messages.chunk.is_empty() {
            break;
        }

        fetched.extend(messages.chunk);

        if messages.end.is_none() {
            break;
        }

        if let Some(token) = messages.end {
            options = MessagesOptions::backward().from(token.as_str());
        } else {
            break;
        }
    }

    Ok(fetched)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: after implementing clap, add a flag for this with different levels
    // also an output path
    let log_file = tracing_appender::rolling::never(".", "met-export-log.txt");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(non_blocking)
        .init();

    // Prompt user for account data
    let user = UserInfo::prompt_user_info().await?;

    // Log in and synchronize state
    println!("Logging in...");
    let client = utils::login(&user).await?;
    client.sync_once(SyncSettings::default()).await?;

    // Import E2EE keys
    println!("Importing keys...");
    let keys = client
        .encryption()
        .import_room_keys((&user.keys_file).into(), &user.keys_pass)
        .await?;
    println!(
        "Imported {} keys out of {}",
        keys.imported_count, keys.total_count
    );

    // Prompt room selection and wait
    let selected_rooms = utils::select_room(&client).await?;

    // Iterate over selected rooms
    for room_id in selected_rooms {
        let room = client.get_room(&room_id).unwrap();
        let room_messages = fetch_all_messages(&client, &room_id).await?;
        // Unwrap should be safe here, coz the cache gets filled during selection
        println!("Room messages for {}", &room.cached_display_name().unwrap());
        println!("---------------------------------------------------\n");

        // TODO: Move the output part somewhere else;
        // Also implement outputting serialized with a data format
        // Then output to a file.
        for message in room_messages {
            if let TimelineEventKind::Decrypted(decrypted_event) = &message.kind {
                if let Ok(message_event) = decrypted_event
                    .event
                    .deserialize_as::<SyncRoomMessageEvent>()
                {
                    if let SyncMessageLikeEvent::Original(original_event) = message_event {
                        println!(
                            "{:?} — {}: {}",
                            original_event.origin_server_ts,
                            original_event.sender,
                            original_event.content.body()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
