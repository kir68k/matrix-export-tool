use matrix_sdk::{
    deserialized_responses::{TimelineEvent, TimelineEventKind},
    room::MessagesOptions,
    Room,
};
use matrix_sdk::{
    ruma,
    ruma::events::{MessageLikeEvent, room::message::RoomMessageEvent},
};

use tokio::{fs::File, io::AsyncWriteExt};

use promkit::crossterm::style::Stylize;

/// Fetch all available message chunks from server
async fn fetch_chunks(room: &Room) -> anyhow::Result<Vec<TimelineEvent>> {
    let mut options = MessagesOptions::backward();
    let mut result: Vec<TimelineEvent> = Vec::new();
    // make name pretty
    let name = room
        .cached_display_name()
        .unwrap()
        .to_string()
        .bold()
        .white();

    // TODO: Rate limiting?
    // Not sure how expensive these requests are for a server...
    loop {
        options.limit = ruma::UInt::from(100u8);

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
