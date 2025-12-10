mod cli;
mod utils;

use std::env;
use std::io::{self, Write};
use std::{io::stdout, time::Duration};

use cli::interface::UserInfo;
use cli::prompts;

use config::Config;
use matrix_sdk::LoopCtrl;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::presence::PresenceState;
use promkit::core::crossterm::{
    ExecutableCommand, cursor,
    style::Stylize,
    terminal::{Clear, ClearType},
};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::Level;
use utils::cache::user_cache::ExportCache;

// Keeping this for later as a reminder.
// (force shutdown)
// const EXIT_TIMEOUT: u64 = 10;

/// Prompt delay, for readability (in ms)
const P_DELAY: Duration = Duration::from_millis(750);

/// Load user config, either by prompt or config file
async fn load_config() -> anyhow::Result<UserInfo> {
    let settings = Config::builder()
        .add_source(config::File::with_name("./met-config.toml"))
        .add_source(config::Environment::with_prefix("MET"))
        .build();

    if let Ok(file) = settings {
        println!("{}", "Loading from config, skipping prompt.".yellow());
        std::thread::sleep(P_DELAY);

        UserInfo::from_config(file)
    } else {
        println!("{}", "Config not found, prompting.".yellow());
        std::thread::sleep(P_DELAY);

        UserInfo::from_prompt().await
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // for debugging.
    let args: Vec<String> = env::args().collect();
    let loglevel = match args[1].as_str() {
        "--debug" | "-d" => Level::DEBUG,
        _ => Level::INFO,
    };

    let log_file = tracing_appender::rolling::never(".", "met-export.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);
    tracing_subscriber::fmt()
        .with_max_level(loglevel)
        .with_writer(non_blocking)
        .init();

    let token = CancellationToken::new();
    let user = load_config().await?;
    let client = utils::client::login(&user).await?;

    // Background sync task (might be expanded in the future, probably with gui?)
    {
        let sync_client = client.clone();
        tokio::task::spawn(async move {
            // fyi, this is the same as `sync`, but using that hit the recursion limit after updating
            // to 0.16.0. This used to also run inside select! with a CancellationToken, which
            // might've increased recursion.
            sync_client.sync_with_callback(
                SyncSettings::new().set_presence(PresenceState::Unavailable),
                |_| async {
                    LoopCtrl::Continue
                }
            ).await
        });
    }

    let main_client = client.clone();
    let main_token = token.clone();
    let main_task: JoinHandle<anyhow::Result<()>> = tokio::task::spawn(async move {
        // Import E2EE keys
        println!("Importing keys...");
        let mut keys = main_client
            .encryption()
            .import_room_keys((&user.keys_file).into(), &user.keys_pass)
            .await?;
        println!(
            "Imported {} keys out of {}",
            keys.imported_count, keys.total_count
        );

        // quick hack for testing, should be kept but improved.
        print!("Additional key files (comma-separated, Enter to skip): ");
        io::stdout().flush().ok();
        let mut extra = String::new();
        if io::stdin().read_line(&mut extra).is_ok() {
            for path in extra.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                let p = promkit::preset::password::Password::default()
                    .title(format!("Password for {}", path))
                    .run()
                    .await?;

                let res = main_client
                    .encryption()
                    .import_room_keys(path.into(), p.as_ref())
                    .await?;
                keys.imported_count += res.imported_count;
                keys.total_count += res.total_count;
            }
        }

        println!(
            "Imported {} keys out of {} (including duplicates)",
            keys.imported_count, keys.total_count
        );

        println!("Verifying client...");
        if !utils::client::verify_client(&main_client).await? {
            println!("{}", "Skipping verification".yellow());
        }

        let selected_rooms = prompts::select_room(&main_client).await?;
        // (note) export data cache for all rooms.
        // it has to be added here and cloned into the export tasks.
        // the clones all point to the same `Arc<Mutex>`.
        let cache = ExportCache::import_cache();

        let mut export_tasks = JoinSet::new();
        for room_id in selected_rooms {
            let ref_cache = cache.clone();
            let ref_client = main_client.clone();
            let room = main_client.get_room(&room_id).unwrap();

            #[rustfmt::skip]
            export_tasks.spawn(async move {
                utils::export::export_room(&ref_client, room, ref_cache).await
            });
        }

        loop {
            tokio::select! {
                _ = main_token.cancelled() => {
                    println!("{}", "Cancelled export tasks.".green().italic());
                    break Ok(());
                }
                result = export_tasks.join_next() => {
                    match result {
                        Some(_) => continue,
                        None => {
                            println!("{}", "All exports finished.".green().italic());
                            break Ok(());
                        }
                    }
                }
            }
        }
    });

    // Weirdly enough, this always selects the 2nd branch.
    // Same with the loop in main_task.
    // update: nvm this might be due to crossterm and prompts.
    // that should be looked at.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\n{}", "Ctrl-c received, exiting...".yellow());
            token.cancel();
        }
        _ = main_task => {
            println!("{}", "All tasks finished.".green().italic());
        }
    }

    // clear terminal
    stdout().execute(Clear(ClearType::All))?;
    stdout().execute(Clear(ClearType::Purge))?;
    stdout().execute(cursor::MoveTo(0, 0))?;
    println!("{}", "Logging out...".yellow().italic());
    match client.logout().await {
        Ok(_) => println!("{}", "Logged out".italic().green()),
        Err(e) => eprintln!("{} {}", "Error logging out:".italic().red(), e),
    }

    Ok(())
}
