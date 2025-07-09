use anyhow::{anyhow, Result};
use crate::cli::interface::UserInfo;

use matrix_sdk::{ruma, Client};
use matrix_sdk::stream::StreamExt;
use matrix_sdk::encryption::{
    verification,
    verification::{SasState, VerificationRequestState},
};
use matrix_sdk::ruma::events::key::verification::VerificationMethod;

use promkit::preset::confirm::Confirm;
use promkit::crossterm::style::Stylize;

/// Log-in using a password and create a client
pub async fn login(user: &UserInfo) -> Result<Client> {
    let uid = ruma::UserId::parse(&user.userid)?;

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

/// Verify with cross-signing
pub async fn verify_client(client: &Client) -> Result<bool> {
    let p = Confirm::new("Start verification?").prompt()?.run()?;
    match p.as_str() {
        "y" => println!("{}", "Starting verification".bold().italic()),
        _ => return anyhow::Ok(false),
    }

    // verify using own user identity
    let identity = client
        .encryption()
        .request_user_identity(client.user_id().unwrap())
        .await?
        .ok_or(anyhow!("Failed to get user identity"))?;

    // TODO: Add QR
    let request = identity.request_verification_with_methods(
        vec![VerificationMethod::SasV1, VerificationMethod::ReciprocateV1]
    ).await?;
    let mut req_stream = request.changes();

    while let Some(state) = req_stream.next().await {
        match state {
            VerificationRequestState::Ready { .. } => {
                println!("{}", "Request ready".yellow());
                break;
            }
            VerificationRequestState::Cancelled(cancel_info) => {
                eprintln!(
                    "{} {}",
                    "Request cancelled, reason:".italic().red(),
                    cancel_info.reason()
                );
                break;
            }
            VerificationRequestState::Done => {
                println!("{}", "Verification completed".green());
                break;
            }
            _ => (),
        }
    }

    // TODO: Add QR
    if let Some(methods) = request.their_supported_methods() {
        if methods.contains(&VerificationMethod::SasV1) {
            println!("Verifying by emoji");
            verify_sas(request).await?;
        } else {
            eprintln!("{}", "Other device doesn't support emoji requests.".italic().red());
            return anyhow::Ok(false);
        }
    }

    anyhow::Ok(true)
}


/// Helper for SAS verification flow
async fn verify_sas(req: verification::VerificationRequest) -> Result<bool> {
    let sas = req
        .start_sas()
        .await?
        .ok_or_else(|| anyhow!("Failed to start emoji verification"))?;

    while let Some(state) = sas.changes().next().await {
        match state {
            SasState::KeysExchanged {
                emojis,
                decimals: _,
            } => {
                let e = emojis.expect("Emoji support required");
                println!("----- Emoji verification -----");
                println!("{}", verification::format_emojis(e.emojis));

                let p = Confirm::new("Do these match on both devices?")
                    .prompt()?
                    .run()?;

                match p.as_str() {
                    "y" => sas.confirm().await?,
                    _ => sas.cancel().await?,
                }
            }
            SasState::Done { .. } => {
                let device = sas.other_device();
                println!(
                    "{} {} ({})",
                    "Verified with:".green().bold(),
                    device.display_name().unwrap_or("no display name"),
                    device.device_id()
                );
                break;
            }
            SasState::Cancelled(cancel_info) => {
                eprintln!(
                    "{} {}",
                    "Request cancelled, reason:".italic().red(),
                    cancel_info.reason()
                );

                return anyhow::Ok(false);
            }
            _ => (),
        }
    }

    anyhow::Ok(true)
}