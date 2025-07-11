use anyhow::anyhow;
use std::{path::PathBuf, time::Duration};

use promkit::crossterm::{
    ExecutableCommand, cursor,
    style::{Attribute, Color, ContentStyle, Stylize},
};
use promkit_derive::Promkit;
use std::io::stdout;
use tokio::time::sleep;

/// Initial information of a user
/// The fields are filled through a prompt
#[derive(Default, Debug, Promkit)]
pub struct UserInfo {
    /// Full user ID
    ///
    /// Example: @user:example.org
    #[form(
        label = "Matrix user ID",
        label_style = ContentStyle {
            foreground_color: Some(Color::DarkYellow),
            ..Default::default()
        },
    )]
    pub userid: String,

    /// Password for the given user ID
    #[form(
        mask = Some('*'),
        label = "Account password",
        label_style = ContentStyle {
            foreground_color: Some(Color::DarkYellow),
            ..Default::default()
        },
    )]
    pub password: String,

    /// Path to the E2EE key file
    ///
    /// Can be relative or absolute
    #[form(
        label = "E2EE Room keys file",
        label_style = ContentStyle {
            foreground_color: Some(Color::DarkYellow),
            ..Default::default()
        }
    )]
    pub keys_file: String,

    /// Passphrase for the given E2EE key file
    #[form(
        mask = Some('*'),
        label = "E2EE keys passphrase",
        label_style = ContentStyle {
            foreground_color: Some(Color::DarkYellow),
            ..Default::default()
        }
    )]
    pub keys_pass: String,
}

impl UserInfo {
    /// Prompt the user to fill out a new [`UserInfo`].
    pub async fn prompt_user_info() -> Result<Self, anyhow::Error> {
        stdout().execute(cursor::SavePosition)?;

        let title = "Press Up/Down to pick, Enter to confirm."
            .with(Color::White)
            .attribute(Attribute::Underlined)
            .attribute(Attribute::Bold);

        // Runs a loop setting a title, prompting the user
        // Returns UserInfo
        let user_info = loop {
            println!("{}", title);
            let mut res = Self::default();

            // [`Box<dyn std::error::Error>`] is inconvenient, so convert it
            if let Err(e) = res.build() {
                return Err(anyhow!("{}", e));
            }

            let keys_valid = PathBuf::from(&res.keys_file).try_exists()?;

            if res.any_empty() || !keys_valid {
                println!("Empty field given or invalid path .\n");
                sleep(Duration::from_millis(500)).await;
                stdout().execute(cursor::RestorePosition)?;
            } else {
                break res;
            }
        };

        Ok(user_info)
    }

    /// Checks if any fields of [`UserInfo`] are empty.
    pub fn any_empty(&self) -> bool {
        [&self.userid, &self.password, &self.keys_file]
            .iter()
            .any(|input| input.is_empty())
    }
}
