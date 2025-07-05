mod cli;
mod utils;

use cli::interface::UserInfo;

use matrix_sdk::config::SyncSettings;
use promkit::crossterm::style::Stylize;
use tokio::task::JoinSet;

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

    // export selected rooms concurrently
    let mut tasks = JoinSet::new();
    for room_id in selected_rooms {
        let room = client.get_room(&room_id).unwrap();
        tasks.spawn(async move {
            if let Err(err) = utils::export_room(&room).await {
                eprintln!("{} {err}", "Export error:".red().bold());
            }
        });
    }
    tasks.join_all().await;

    println!("{}", "Logging out".italic().yellow());
    client.logout().await?;

    Ok(())
}
