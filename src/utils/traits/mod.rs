use std::{env::temp_dir, io::Write as StdWrite, path::PathBuf, sync::{Arc, Mutex}};

use anyhow::anyhow;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters},
    ruma::events::{self, room::message},
};
use std::fs::OpenOptions;

/// Temporary buffer for text messages.
pub struct TextBufferInner {
    pub lines: Vec<String>,
}

#[derive(Clone)]
pub struct TextBuffer {
    inner: Arc<Mutex<TextBufferInner>>,
    pub file: PathBuf,
}

impl TextBufferInner {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
        }
    }
}

impl TextBuffer {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        let buf = Mutex::new(TextBufferInner::new());
        Self {
            inner: Arc::new(buf),
            file: file.into(),
        }
    }

    fn push_line(&self, line: String) {
        let mut lock = self.inner.try_lock();
        if let Ok(ref mut inner) = lock {
            inner.lines.push(line);
        } else {
            tracing::error!("Couldn't get mutex lock for TextBufferInner.");
        }
    }

    pub fn write(&self) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file)?;

        let mut lock = self.inner.lock();
        match lock {
            Ok(ref mut inner) => {
                let output = std::mem::take(&mut inner.lines)
                    .into_iter()
                    .collect::<String>();

                file.write_all(output.as_bytes())?;
                file.flush()?;

                anyhow::Ok(())
            }
            Err(e) => {
                tracing::error!("TextBuffer::write | Mutex lock error: {e}");
                anyhow::bail!("Mutex lock error: {e}");
            }
        }
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
        buffer: &TextBuffer,
    ) -> anyhow::Result<()>;
}

impl<C> ValidEvent<C> for message::TextMessageEventContent
where
    C: events::MessageLikeEventContent,
{
    async fn process_event(
        &self,
        orig_ev: &events::OriginalMessageLikeEvent<C>,
        buffer: &TextBuffer,
    ) -> anyhow::Result<()> {

        let formatted = format!(
            "{:?} - {}: {}\n",
            orig_ev.origin_server_ts, orig_ev.sender, self.body
        );

        buffer.push_line(formatted);

        anyhow::Ok(())
    }
}

/// Trait for working with *decrypted* media events (e.g. images).
///
/// Types implementing this are processed and sent for exporting.
pub trait ValidMediaEvent<C>
where
    C: events::MessageLikeEventContent
{
    async fn process_event(
        &self,
        ev: &events::OriginalMessageLikeEvent<C>,
        client: &Client,
        media_dir: &PathBuf
    ) -> anyhow::Result<()>;
}

impl<C> ValidMediaEvent<C> for message::FileMessageEventContent
where
    C: events::MessageLikeEventContent
{
    async fn process_event(
        &self,
        ev: &events::OriginalMessageLikeEvent<C>,
        client: &Client,
        media_dir: &PathBuf
    ) -> anyhow::Result<()> {
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

        let mut res_path = media_dir.join(self.filename());
        if let Ok(true) = res_path.try_exists() {
            let new = format!("{:?} - {}", ev.origin_server_ts, self.filename());
            res_path.set_file_name(new);
        }

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
                    tracing::info!(
                        "Media: Saved file {} to {} (size: {} KiB)",
                        self.filename(),
                        &res_path.display(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {e}",
                        handle.path().display()
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {e}"));
            }
        }
    }
}

impl<C> ValidMediaEvent<C> for message::ImageMessageEventContent
where
    C: events::MessageLikeEventContent
{
    async fn process_event(
        &self,
        ev: &events::OriginalMessageLikeEvent<C>,
        client: &Client,
        media_dir: &PathBuf
    ) -> anyhow::Result<()> {
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

        let mut res_path = media_dir.join(self.filename());
        if let Ok(true) = res_path.try_exists() {
            let new = format!("{:?} - {}", ev.origin_server_ts, self.filename());
            res_path.set_file_name(new);
        }

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
                    tracing::info!(
                        "Media: Saved image {} to {} (size: {} KiB)",
                        self.filename(),
                        &res_path.display(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {e}",
                        handle.path().display()
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {e}"));
            }
        }
    }
}

impl<C> ValidMediaEvent<C> for message::VideoMessageEventContent
where
    C: events::MessageLikeEventContent
{
    async fn process_event(
        &self,
        ev: &events::OriginalMessageLikeEvent<C>,
        client: &Client,
        media_dir: &PathBuf
    ) -> anyhow::Result<()> {
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

        let mut res_path = media_dir.join(self.filename());
        if let Ok(true) = res_path.try_exists() {
            let new = format!("{:?} - {}", ev.origin_server_ts, self.filename());
            res_path.set_file_name(new);
        }

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
                    tracing::info!(
                        "Media: Saved video {} to {} (size: {} KiB)",
                        self.filename(),
                        &res_path.display(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {e}",
                        handle.path().display()
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {e}"));
            }
        }
    }
}

impl<C> ValidMediaEvent<C> for message::AudioMessageEventContent
where
    C: events::MessageLikeEventContent
{
    async fn process_event(
        &self,
        ev: &events::OriginalMessageLikeEvent<C>,
        client: &Client,
        media_dir: &PathBuf
    ) -> anyhow::Result<()> {
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

        let mut res_path = media_dir.join(self.filename());
        if let Ok(true) = res_path.try_exists() {
            let new = format!("{:?} - {}", ev.origin_server_ts, self.filename());
            res_path.set_file_name(new);
        }

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
                    tracing::info!(
                        "Media: Saved audio {} to {} (size: {} KiB)",
                        self.filename(),
                        &res_path.display(),
                        (size / 1024)
                    );
                    return anyhow::Ok(());
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error copying from {} ---- {e}",
                        handle.path().display()
                    ));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Request handle error: {e}"));
            }
        }
    }
}
