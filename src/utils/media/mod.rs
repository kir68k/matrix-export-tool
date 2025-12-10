pub mod text;

use anyhow::anyhow;
use matrix_sdk::{Client, media::{MediaFormat, MediaRequestParameters}, ruma::{
    MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedUserId,
    events::{
        AnyMessageLikeEventContent, AnySyncMessageLikeEvent, AnySyncStateEvent,
        AnySyncTimelineEvent, AnyTimelineEvent,
        room::{
            MediaSource, avatar::RoomAvatarEventContent, member::RoomMemberEventContent,
            message::MessageType,
        },
    },
}, stream::StreamExt};

use crate::utils::cache::output_cache::WriteDirs;

/// How many files to concurrently download.
///
/// This *should* get around any rate limits.
// TODO: add and test proper rate limiting...
const MEDIA_DOWNLOAD_RATE: usize = 8;

/// Metadata common to all events.
#[derive(Clone, Debug)]
pub struct EventMetadata {
    pub sender: OwnedUserId,
    pub timestamp: MilliSecondsSinceUnixEpoch,
    pub event_id: OwnedEventId,
}

/// The kind of media, used for directories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaKind {
    File,
    Image,
    Video,
    Audio,
    UserAvatar,
    RoomAvatar,
}

impl MediaKind {
    /// Returns the subdirectory name for this media kind.
    pub fn subdir(&self) -> &'static str {
        match self {
            Self::File => "Files",
            Self::Image => "Images",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::UserAvatar => "Avatars/Users",
            Self::RoomAvatar => "Avatars/Rooms",
        }
    }

    pub fn subdirs() -> [&'static str; 6] {
        [
            "Files",
            "Images",
            "Video",
            "Audio",
            "Avatars/Users",
            "Avatars/Rooms",
        ]
    }
}

/// Type for any valid media-like event.
#[derive(Clone, Debug)]
pub struct DownloadableMedia {
    pub source: MediaSource,
    pub filename: String,
    pub mimetype: Option<String>,
    pub kind: MediaKind,
    pub metadata: EventMetadata,
}

impl DownloadableMedia {
    /// Create from a room message media type.
    // todo: could be improved later mby, works though.
    fn from_message_type(msgtype: &MessageType, metadata: EventMetadata) -> Option<Self> {
        match msgtype {
            MessageType::File(file) => Some(Self {
                source: file.source.clone(),
                filename: file.filename().to_owned(),
                mimetype: file.info.as_ref().and_then(|i| i.mimetype.clone()),
                kind: MediaKind::File,
                metadata,
            }),
            MessageType::Image(image) => Some(Self {
                source: image.source.clone(),
                filename: image.filename().to_owned(),
                mimetype: image.info.as_ref().and_then(|i| i.mimetype.clone()),
                kind: MediaKind::Image,
                metadata,
            }),
            MessageType::Video(video) => Some(Self {
                source: video.source.clone(),
                filename: video.filename().to_owned(),
                mimetype: video.info.as_ref().and_then(|i| i.mimetype.clone()),
                kind: MediaKind::Video,
                metadata,
            }),
            MessageType::Audio(audio) => Some(Self {
                source: audio.source.clone(),
                filename: audio.filename().to_owned(),
                mimetype: audio.info.as_ref().and_then(|i| i.mimetype.clone()),
                kind: MediaKind::Audio,
                metadata,
            }),
            _ => None,
        }
    }

    /// Create from a user avatar (m.room.member) event.
    fn from_member_avatar(
        content: &RoomMemberEventContent,
        metadata: EventMetadata,
    ) -> Option<Self> {
        let avatar_url = content.avatar_url.as_ref()?;
        Some(Self {
            source: MediaSource::Plain(avatar_url.clone()),
            // no extension for now, it's added later
            filename: format!("{}", metadata.sender),
            mimetype: None,
            kind: MediaKind::UserAvatar,
            metadata,
        })
    }

    /// Create from a room avatar (m.room.avatar) event.
    fn from_room_avatar(content: &RoomAvatarEventContent, metadata: EventMetadata) -> Option<Self> {
        let avatar_url = content.url.as_ref()?;
        Some(Self {
            source: MediaSource::Plain(avatar_url.clone()),
            filename: format!("room.{:?}", metadata.timestamp),
            mimetype: content.info.as_ref().and_then(|i| i.mimetype.clone()),
            kind: MediaKind::RoomAvatar,
            metadata,
        })
    }
}

/// Types of events which can be processed.
#[derive(Clone, Debug)]
pub enum ProcessableEvent {
    /// Text message content.
    Text {
        body: String,
        metadata: EventMetadata,
    },
    /// Downloadable media content.
    Media(DownloadableMedia),
}

impl ProcessableEvent {
    pub fn try_from_sync(event: AnySyncTimelineEvent) -> Option<Self> {
        match event {
            AnySyncTimelineEvent::MessageLike(msg) => Self::from_sync_message_like(msg),
            AnySyncTimelineEvent::State(state) => Self::from_sync_state(state),
        }
    }

    /// Convenience fn, converts into a sync event and calls [`Self::try_from_sync`].
    ///
    /// Note that the room ID is lost upon conversion, but the raw/serialized ev still has it.
    pub fn try_from_full(event: AnyTimelineEvent) -> Option<Self> {
        let sync_event: AnySyncTimelineEvent = event.into();
        Self::try_from_sync(sync_event)
    }

    fn from_sync_message_like(event: AnySyncMessageLikeEvent) -> Option<Self> {
        let orig = event.original_content()?;

        if let AnyMessageLikeEventContent::RoomMessage(msg) = orig {
            let metadata = EventMetadata {
                sender: event.sender().to_owned(),
                timestamp: event.origin_server_ts(),
                event_id: event.event_id().to_owned(),
            };

            match &msg.msgtype {
                MessageType::Text(text) => Some(Self::Text {
                    body: text.body.clone(),
                    metadata,
                }),
                other => DownloadableMedia::from_message_type(other, metadata).map(Self::Media),
            }
        } else {
            None
        }
    }

    fn from_sync_state(event: AnySyncStateEvent) -> Option<Self> {
        let metadata = EventMetadata {
            sender: event.sender().to_owned(),
            timestamp: event.origin_server_ts(),
            event_id: event.event_id().to_owned(),
        };

        match event {
            AnySyncStateEvent::RoomMember(member_ev) => {
                let orig = member_ev.as_original()?;
                DownloadableMedia::from_member_avatar(&orig.content, metadata).map(Self::Media)
            }
            AnySyncStateEvent::RoomAvatar(avatar_ev) => {
                let orig = avatar_ev.as_original()?;
                DownloadableMedia::from_room_avatar(&orig.content, metadata).map(Self::Media)
            }
            _ => None,
        }
    }
}

/// Trait for processing of media events.
pub trait ProcessableMediaEvent {
    async fn send_to_process(&mut self, client: &Client, dirs: &WriteDirs);
}

impl ProcessableMediaEvent for Vec<DownloadableMedia> {
    async fn send_to_process(&mut self, client: &Client, dirs: &WriteDirs) {
        tracing::info!("Downloading {} media files.", self.len());
        tokio_stream::iter(self)
            .for_each_concurrent(MEDIA_DOWNLOAD_RATE, |media| async move {
                tracing::info!(
                    "Media: Received {} {}, starting download.",
                    media.kind.subdir(),
                    media.filename
                );

                if let Err(e) = download_media(media, client, dirs).await {
                    tracing::error!(
                        "Media kind: {:?} | Err: {e} | Event ID: {} | Event timestamp: {:?}",
                        media.kind,
                        media.metadata.event_id,
                        media.metadata.timestamp
                    );
                }

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            })
            .await;
    }
}

impl ProcessableMediaEvent for Vec<ProcessableEvent> {
    async fn send_to_process(&mut self, client: &Client, dirs: &WriteDirs) {
        let mut media_events: Vec<_> = self
            .iter()
            .filter_map(|ev| match ev {
                ProcessableEvent::Media(m) => Some(m.clone()),
                _ => None,
            })
            .collect();
        media_events.send_to_process(client, dirs).await;
    }
}

/// Unified media download function.
///
/// Downloads any `DownloadableMedia`.
// todo: I think this should be a method for DownloadableMedia, for consistency.
pub async fn download_media(
    media: &mut DownloadableMedia,
    client: &Client,
    dirs: &WriteDirs,
) -> anyhow::Result<()> {
    let request = MediaRequestParameters {
        source: media.source.clone(),
        format: MediaFormat::File,
    };

    // This is only set as get_media_file() needs it.
    let content_type = media
        .mimetype
        .as_ref()
        .unwrap_or(&mime::APPLICATION_OCTET_STREAM.to_string())
        .parse::<mime::Mime>()?;

    let temp_dir = std::env::temp_dir();

    let media_dir = dirs.media_dir().join(media.kind.subdir());
    tokio::fs::create_dir_all(&media_dir).await.ok();

    let request_handle = client
        .media()
        .get_media_file(
            &request,
            None,
            &content_type,
            false,
            Some(temp_dir.display().to_string()),
        )
        .await;

    match request_handle {
        Ok(handle) => {
            // The final path, e.g. `PWD/room_name/media/Images/thing.jpg`
            let mut res_path = media_dir.join(&media.filename);
            // The temp path, e.g. `TMPDIR/$some_event_id`
            let temp_path = temp_dir.join(media.metadata.event_id.as_str());

            // This is a hard link and sadly fails if /tmp isn't on the same fs.
            let mut temp_file = match handle.persist(&temp_path) {
                Ok(file) => tokio::fs::File::from_std(file),
                Err(e) => {
                    return Err(anyhow!(
                        "Couldn't persist file. Error: {e}"
                    ))
                }
            };

            // Types like `m.room.avatar` and `m.room.member` don't contain a MIME type or
            // filename, and get_media_content only returns bytes to get_media_file, not HTTP
            // headers which do have those. If the current ev matches these types, read the header
            // through mimetype_detector, and add the extension. Since the final name might have
            // the server tld, this uses `add` instead of `set_extension`.
            if matches!(media.kind, MediaKind::UserAvatar | MediaKind::RoomAvatar) {
                tracing::debug!("Current filename: {}", res_path.display());
                res_path.add_extension(mimetype_detector::detect_file(&temp_path)?.extension().replace(".",""));
                tracing::debug!("New filename: {}", res_path.display());
            }

            // Avoiding unwrap out of principle, pretty sure you can't upload a nameless file
            // though...?
            // Also doing it like browsers/file managers with "Copy (num)" would be nicer but this
            // doesn't need a counter.
            if let Ok(true) = res_path.try_exists()
                && let Some(raw_name) = res_path.file_name()
                && let Some(file_name) = raw_name.to_str()
            {
                res_path.set_file_name(format!("{:?} - {}", media.metadata.timestamp, file_name));
            }

            tracing::debug!("Creating res_file at {}", &res_path.display());
            let mut res_file= tokio::fs::File::options()
                .create_new(true)
                .write(true)
                .open(&res_path)
                .await?;

            tracing::debug!("Copying {} to {}", &temp_path.display(), &res_path.display());
            match tokio::io::copy(&mut temp_file, &mut res_file).await {
                Ok(size) => {
                    tracing::info!(
                        "Media: Saved {} {} to {} (size: {} KiB)",
                        media.kind.subdir(),
                        media.filename,
                        res_path.display(),
                        (size / 1024)
                    );
                    res_file.sync_data().await?;
                    tokio::fs::remove_file(&temp_path).await?;

                    Ok(())
                }
                Err(e) => Err(anyhow!(
                    "Error copying from {} ---- {e}",
                    temp_path.display()
                )),
            }
        }
        Err(e) => Err(anyhow!("Request handle error: {e}")),
    }
}

