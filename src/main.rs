mod cli;
mod utils;

use cli::interface::UserInfo;
use cli::prompts;

use matrix_sdk::config::SyncSettings;
use promkit::crossterm::style::Stylize;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO: after implementing clap, add a flag for this with different levels
    // also an output path
    let log_file = tracing_appender::rolling::never(".", "met-export-log.txt");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(non_blocking)
        .init();

    // for graceful shutdown
    let exit_token = CancellationToken::new();

    // Prompt user for account data
    let user = UserInfo::prompt_user_info().await?;

    // Log in and synchronize state
    println!("Logging in...");
    let client = utils::client::login(&user).await?;

    // background sync
    // TODO: this is rlly only needed during verification, redo?
    let client_cloned = client.clone();
    let sync_token = exit_token.clone();
    let sync_handle = tokio::task::spawn(async move {
        tokio::select! {
            result = client_cloned.sync(SyncSettings::default()) => {
                if let Err(e) = result {
                    eprintln!("Sync error: {}", e);
                }
            }
            _ = sync_token.cancelled() => {
                println!("Background sync stopped");
            }
        }
    });

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

    println!("Verifying client...");
    if !utils::client::verify_client(&client).await? {
        println!("{}", "Skipping verification".yellow());
    }

    // Prompt room selection and wait
    let selected_rooms = prompts::select_room(&client).await?;

    // export selected rooms concurrently
    let mut tasks = JoinSet::new();
    for room_id in selected_rooms {
        let room = client.get_room(&room_id).unwrap();
        let export_token = exit_token.clone();
        tasks.spawn(async move {
            if let Err(err) = utils::export::export_room(room, export_token).await {
                eprintln!("{} {err}", "Export error:".red().bold());
            }
        });
    }

    loop {
        tokio::select! {
            result = tasks.join_next() => {
                match result {
                    Some(_) => {
                        continue;
                    }
                    None => {
                        println!("{}", "All exports completed".green());
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\n{}", "Ctrl-C received, exiting gracefully".yellow());
                exit_token.cancel();

                // 10 sec timeout
                // todo: this should be displayed.
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    std::process::exit(1);
                });
            }
        }
    }

    sync_handle.abort();

    println!("{}", "Logging out".italic().yellow());
    match client.logout().await {
        Ok(_) => println!("{}", "Logged out".italic().green()),
        Err(e) => eprintln!("{} {}", "Error logging out:".italic().red(), e),
    }

    Ok(())
}
