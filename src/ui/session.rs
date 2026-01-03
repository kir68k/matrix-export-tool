use std::path::PathBuf;

use anyhow::Context as ErrorContext;
use matrix_sdk::{
    Client, OwnedServerName, ServerName, SessionMeta,
    authentication::{SessionTokens, matrix::MatrixSession},
    ruma,
};

use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::ui::{APP_ID, ExportUser};

/// Data needed to rebuild the client
///
/// The database password lives as an OS secret, using [`keyring_core::Entry`]
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSession {
    homeserver: OwnedServerName,
    db_path: PathBuf,
}

/// Intermediatry version of [`MatrixSession`] without the access token.
///
/// The access token lives as an OS secret, using [`keyring_core::Entry`],
/// and is retrieved upon restoration.
#[derive(Debug, Serialize, Deserialize)]
struct SanitizedMatrixSession {
    meta: SessionMeta,
    refresh_token: Option<String>,
}

/// Full session that can be stored in the data directory.
#[derive(Debug, Serialize, Deserialize)]
struct FullSession {
    /// Data to rebuild the client
    client_session: ClientSession,

    /// Matrix user session (without access token)
    user_session: SanitizedMatrixSession,
    // /// Latest Sync token, used to skip unnecessary init sync.
    // #[serde(skip_serializing_if = "Option::is_none")]
    // sync_token: Option<String>,
}

/// Trait for user types which can implement persistent sessions.
///
/// Mostly made for future use, when CLI works again.
pub trait UserSession {
    /// Build necessary client and session data.
    async fn build_client(
        server_name: &ServerName,
        db_path: &PathBuf,
        db_passphrase: &String,
    ) -> anyhow::Result<Client>;
    /// Restore a session, e.g. from a session file.
    async fn restore_session(&self) -> anyhow::Result<(Client, String)>;
    /// Log in, creating a new client, or possibly restoring an existing one.
    async fn login(&self) -> anyhow::Result<(Client, Option<String>)>;
    /// Log out, removing the OS secrets.
    async fn logout(&self) -> anyhow::Result<()>;
    // #[allow(dead_code)]
    // /// Update the stored sync token to restore from, if any.
    // async fn update_sync_token(&self, sync_token: String) -> anyhow::Result<()>;
}

/// Keyring entry prefix for a database key
const DB_PASSPHRASE_KEY: &str = "db_passphrase";
/// Keyring entry prefix for an access token
const ACCESS_TOKEN_KEY: &str = "access_token";

/// Retrieve a secret in the OS keyring
async fn get_secret_from_keyring(userid: impl ToString, key: &str) -> anyhow::Result<String> {
    let userid = userid.to_string();
    let key = key.to_string();

    let entry_name = format!("{}:{}", key, userid);
    let db_entry = keyring_core::Entry::new(APP_ID, &entry_name)?;
    let secret = db_entry.get_secret()?;

    Ok(String::from_utf8(secret)?)
}

/// Store a secret in the OS keyring
async fn store_secret_in_keyring(
    userid: impl ToString,
    key: &str,
    secret: String,
) -> anyhow::Result<()> {
    let userid = userid.to_string();
    let key = key.to_string();

    let entry_name = format!("{}:{}", key, userid);
    let db_entry = keyring_core::Entry::new(APP_ID, &entry_name)?;
    db_entry.set_secret(secret.as_bytes())?;

    Ok(())
}

/// Remove a secret from the OS keyring
async fn delete_secret_from_keyring(userid: impl ToString, key: &str) -> anyhow::Result<()> {
    let userid = userid.to_string();
    let key = key.to_string();

    let entry_name = format!("{}:{}", key, userid);
    let db_entry = keyring_core::Entry::new(APP_ID, &entry_name)?;
    let _ = db_entry.delete_credential();

    Ok(())
}

fn build_client_session(user: &ExportUser) -> anyhow::Result<ClientSession> {
    let ExportUser {
        userid, data_dir, ..
    } = &user;

    let mut rng = rng();

    let db_subdir: String = (&mut rng)
        .sample_iter(Alphanumeric)
        .take(7)
        .map(char::from)
        .collect();
    let db_path = data_dir.join(db_subdir);

    let user_id = ruma::UserId::parse(userid)?;

    Ok(ClientSession {
        homeserver: user_id.server_name().to_owned(),
        db_path,
    })
}

fn generate_db_passphrase() -> String {
    let mut rng = rng();

    (&mut rng)
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect::<String>()
}

impl UserSession for ExportUser {
    async fn restore_session(&self) -> anyhow::Result<(Client, String)> {
        // read session file
        let serialized = fs::read_to_string(&self.session_file).await?;
        let FullSession {
            client_session,
            user_session,
            // sync_token,
        } = serde_json::from_str(&serialized)?;
        let user_id = user_session.meta.user_id.to_string();

        // retrieve both secrets from keyring
        let db_passphrase =
            get_secret_from_keyring(&user_session.meta.user_id, DB_PASSPHRASE_KEY).await?;
        let access_token =
            get_secret_from_keyring(&user_session.meta.user_id, ACCESS_TOKEN_KEY).await?;

        let full_user_session = MatrixSession {
            meta: user_session.meta,
            tokens: SessionTokens {
                access_token: access_token.into(),
                refresh_token: user_session.refresh_token,
            },
        };

        let client = Self::build_client(
            &client_session.homeserver,
            &client_session.db_path,
            &db_passphrase,
        )
        .await?;

        client.restore_session(full_user_session).await?;

        Ok((client, user_id))
    }

    async fn login(&self) -> anyhow::Result<(Client, Option<String>)> {
        let Self {
            userid,
            password,
            session_file,
            data_dir,
            ..
        } = &self;

        fs::create_dir_all(data_dir).await?;

        let db_passphrase = generate_db_passphrase();
        let client_session = build_client_session(&self)?;
        let client = Self::build_client(
            &client_session.homeserver,
            &client_session.db_path,
            &db_passphrase,
        )
        .await?;
        let matrix_auth = client.matrix_auth();

        matrix_auth
            .login_username(&userid, &password)
            .initial_device_display_name("matrix-export-tool")
            .await?;

        let matrix_session = matrix_auth
            .session()
            .ok_or_else(|| anyhow::anyhow!("Failed to get user session"))?;

        // gets saved as an OS secret later
        let access_token = matrix_session.tokens.access_token.to_string();

        let sanitized_session = SanitizedMatrixSession {
            meta: matrix_session.meta.clone(),
            refresh_token: matrix_session.tokens.refresh_token.clone(),
        };

        let full_session = FullSession {
            client_session,
            user_session: sanitized_session,
            // sync_token: None,
        };
        let serialized = serde_json::to_string(&full_session)?;
        fs::write(session_file, serialized).await?;

        let user_id = &full_session.user_session.meta.user_id;
        store_secret_in_keyring(user_id, DB_PASSPHRASE_KEY, db_passphrase).await?;
        store_secret_in_keyring(user_id, ACCESS_TOKEN_KEY, access_token).await?;

        Ok((client, None))
    }

    async fn build_client(
        server_name: &ServerName,
        db_path: &PathBuf,
        db_passphrase: &String,
    ) -> anyhow::Result<Client> {
        #[cfg(feature = "http_debug")]
        let client = client_custom_tls(server_name, db_path, db_passphrase).await?;

        #[cfg(not(feature = "http_debug"))]
        let client = Client::builder()
            .server_name(server_name)
            .sqlite_store(db_path, Some(db_passphrase))
            .build()
            .await?;

        Ok(client)
    }

    async fn logout(&self) -> anyhow::Result<()> {
        if fs::try_exists(&self.session_file).await? {
            fs::remove_file(&self.session_file).await?;
        }

        // I'd rather use ClientSession for consistency but I'm not sure if ExportUser should have it
        delete_secret_from_keyring(&self.userid, DB_PASSPHRASE_KEY).await?;
        delete_secret_from_keyring(&self.userid, ACCESS_TOKEN_KEY).await?;

        self.client
            .as_ref()
            .clone()
            .context("ERR logging out: No client, somehow?")?
            .logout()
            .await?;

        Ok(())
    }
}

/// I added this cus I couldn't find *any* debug traces for a media request issue*.
/// So, in debug mode, the client will use a custom reqwest/rustls client & config.
/// The only change *should* be enabling `$SSLKEYLOGFILE` in rustls.
/// they disable it by default as to not leak info, iirc.
///
/// *(no server logs either, soooo i thought of decrypting the traffic...)
#[cfg(feature = "http_debug")]
async fn client_custom_tls(
    server_name: &ServerName,
    db_path: &PathBuf,
    db_passphrase: &String,
) -> anyhow::Result<Client> {
    use std::sync::Arc;

    tracing::debug!("`http_debug` enabled: Creating custom TLS client.");
    if std::env::var("SSLKEYLOGFILE").is_ok() {
        tracing::debug!("Envvar SSLKEYLOGFILE is set.");
    } else {
        tracing::debug!(
            "Envvar SSLKEYLOGFILE is not set (or invalid)! Feature `http_debug` doesn't do much without it."
        );
    }

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Couldn't install the aws-lc-rs crypto provider.");

    let root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    tls.key_log = Arc::new(rustls::KeyLogFile::new());

    let reqwest_client = reqwest::ClientBuilder::new()
        .https_only(true)
        .use_preconfigured_tls(tls)
        .build()?;

    let client = Client::builder()
        .server_name(server_name)
        .sqlite_store(db_path, Some(db_passphrase))
        .http_client(reqwest_client)
        .build()
        .await?;

    Ok(client)
}
