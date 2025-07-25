use std::{env::temp_dir, path::PathBuf};

use anyhow::anyhow;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters},
    ruma::events::{self, room::message},
};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use tracing::Level;

/// Directory and filename for a media event
#[derive(Clone)]
pub struct FilePath {
    media_dir: PathBuf,
    filename: Option<String>,
}

impl FilePath {
    pub fn new(media_dir: PathBuf) -> Self {
        Self {
            media_dir,
            filename: None,
        }
    }

    pub fn set_filename(&mut self, name: &str) {
        self.filename.replace(name.to_string());
    }

    fn join_paths(&self) -> PathBuf {
        self.media_dir.join(self.filename())
    }

    fn filename(&self) -> String {
        self.filename.clone().unwrap()
    }
}

/// Trait for working with *decrypted* text (or text-like) events.
///
/// Types implementing this are processed and sent for exporting.
pub trait ValidEvent<C>
where
    C: events::MessageLikeEventContent,
{
    async fn process_event(
        &self,
        orig_ev: &events::OriginalMessageLikeEvent<C>,
        text_path: &FilePath,
    ) -> anyhow::Result<()>;
}

impl<C> ValidEvent<C> for message::TextMessageEventContent
where
    C: events::MessageLikeEventContent,
{
    async fn process_event(
        &self,
        orig_ev: &events::OriginalMessageLikeEvent<C>,
        text_path: &FilePath,
    ) -> anyhow::Result<()> {
        let text_file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(text_path.join_paths())
            .await;

        let formatted_msg = format!(
            "{:?} - (in {}) {}: {}\n",
            orig_ev.origin_server_ts, orig_ev.room_id, orig_ev.sender, self.body
        );

        match text_file {
            Ok(mut file) => {
                file.write_all(formatted_msg.as_bytes()).await?;
                return anyhow::Ok(());
            }
            Err(e) => {
                return Err(anyhow!("Error opening text file: {}", e));
            }
        }
    }
}

/// Trait for working with *decrypted* media events (e.g. images).
///
/// Types implementing this are processed and sent for exporting.
pub trait ValidMediaEvent {
    async fn process_event(&self, client: &Client, media_path: &FilePath) -> anyhow::Result<()>;
}

impl ValidMediaEvent for message::FileMessageEventContent {
    /// Take this event and process it in an appropriate way, making it ready for export.
    ///
    /// - `client`: The current client, used for downloading media.
    ///
    /// - `orig_ev`: The original event, for info not specific to this type.
    ///
    /// - `media_dir`: Room-specific media directory this writes to.
    async fn process_event(&self, client: &Client, media_path: &FilePath) -> anyhow::Result<()> {
        let request = MediaRequestParameters {
            source: self.source.clone(),
            format: MediaFormat::File,
        };

        let content_type = self
            .info
            .as_ref()
            .ok_or(anyhow!("Couldn't get the info for media file."))?
            .mimetype
            .as_ref()
            .unwrap_or(&mime::APPLICATION_OCTET_STREAM.to_string()) // Needed.
            .parse::<mime::Mime>()?;

        let res_path = media_path.join_paths();
        let temp_dir = Some(temp_dir().display().to_string());
        let request_handle = client
            .media()
            .get_media_file(
                &request,
                Some(self.filename().to_string()),
                &content_type,
                false,
                temp_dir,
            )
            .await;

        match request_handle {
            Ok(handle) => match tokio::fs::copy(handle.path(), &res_path).await {
                Ok(size) => {
                    tracing::event!(
                        Level::INFO,
                        "Media: Saved file {} (size: {} KiB)",
                        self.filename(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {}",
                        handle.path().display(),
                        e
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {}", e));
            }
        }
    }
}

impl ValidMediaEvent for message::ImageMessageEventContent {
    /// Take this event and process it in an appropriate way, making it ready for export.
    ///
    /// - `client`: The current client, used for downloading media.
    ///
    /// - `orig_ev`: The original event, for info not specific to this type.
    ///
    /// - `media_dir`: Room-specific media directory this writes to.
    async fn process_event(&self, client: &Client, media_path: &FilePath) -> anyhow::Result<()> {
        let request = MediaRequestParameters {
            source: self.source.clone(),
            format: MediaFormat::File,
        };

        let content_type = self
            .info
            .as_ref()
            .ok_or(anyhow!("Couldn't get the info for media file."))?
            .mimetype
            .as_ref()
            .unwrap_or(&mime::APPLICATION_OCTET_STREAM.to_string()) // Needed.
            .parse::<mime::Mime>()?;

        let res_path = media_path.join_paths();
        let temp_dir = Some(temp_dir().display().to_string());
        let request_handle = client
            .media()
            .get_media_file(
                &request,
                Some(self.filename().to_string()),
                &content_type,
                false,
                temp_dir,
            )
            .await;

        match request_handle {
            Ok(handle) => match tokio::fs::copy(handle.path(), &res_path).await {
                Ok(size) => {
                    tracing::event!(
                        Level::INFO,
                        "Media: Saved image {} (size: {} KiB)",
                        self.filename(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {}",
                        handle.path().display(),
                        e
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {}", e));
            }
        }
    }
}

impl ValidMediaEvent for message::VideoMessageEventContent {
    /// Take this event and process it in an appropriate way, making it ready for export.
    ///
    /// - `client`: The current client, used for downloading media.
    ///
    /// - `orig_ev`: The original event, for info not specific to this type.
    ///
    /// - `media_dir`: Room-specific media directory this writes to.
    async fn process_event(&self, client: &Client, media_path: &FilePath) -> anyhow::Result<()> {
        let request = MediaRequestParameters {
            source: self.source.clone(),
            format: MediaFormat::File,
        };

        let content_type = self
            .info
            .as_ref()
            .ok_or(anyhow!("Couldn't get the info for media file."))?
            .mimetype
            .as_ref()
            .unwrap_or(&mime::APPLICATION_OCTET_STREAM.to_string()) // Needed.
            .parse::<mime::Mime>()?;

        let res_path = media_path.join_paths();
        let temp_dir = Some(temp_dir().display().to_string());
        let request_handle = client
            .media()
            .get_media_file(
                &request,
                Some(self.filename().to_string()),
                &content_type,
                false,
                temp_dir,
            )
            .await;

        match request_handle {
            Ok(handle) => match tokio::fs::copy(handle.path(), &res_path).await {
                Ok(size) => {
                    tracing::event!(
                        Level::INFO,
                        "Media: Saved video {} (size: {} KiB)",
                        self.filename(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {}",
                        handle.path().display(),
                        e
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {}", e));
            }
        }
    }
}

impl ValidMediaEvent for message::AudioMessageEventContent {
    /// Take this event and process it in an appropriate way, making it ready for export.
    ///
    /// - `client`: The current client, used for downloading media.
    ///
    /// - `orig_ev`: The original event, for info not specific to this type.
    ///
    /// - `media_dir`: Room-specific media directory this writes to.
    async fn process_event(&self, client: &Client, media_path: &FilePath) -> anyhow::Result<()> {
        let request = MediaRequestParameters {
            source: self.source.clone(),
            format: MediaFormat::File,
        };

        let content_type = self
            .info
            .as_ref()
            .ok_or(anyhow!("Couldn't get the info for media file."))?
            .mimetype
            .as_ref()
            .unwrap_or(&mime::APPLICATION_OCTET_STREAM.to_string()) // Needed.
            .parse::<mime::Mime>()?;

        let res_path = media_path.join_paths();
        let temp_dir = Some(temp_dir().display().to_string());
        let request_handle = client
            .media()
            .get_media_file(
                &request,
                Some(self.filename().to_string()),
                &content_type,
                false,
                temp_dir,
            )
            .await;

        match request_handle {
            Ok(handle) => match tokio::fs::copy(handle.path(), &res_path).await {
                Ok(size) => {
                    tracing::event!(
                        Level::INFO,
                        "Media: Saved audio {} (size: {} KiB)",
                        self.filename(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {}",
                        handle.path().display(),
                        e
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {}", e));
            }
        }
    }
}
