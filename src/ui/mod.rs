mod init;
pub use init::init;
use tokio::time as async_time;
mod icons;
mod session;
mod states;
mod tasks;
mod titlebar;
mod user;
mod views;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use gpui_component::*;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::media::MediaThumbnailSettings;
use matrix_sdk::{Client, Room, ruma};
use mimetype_detector::{IMAGE_APNG, IMAGE_GIF, IMAGE_JPEG, IMAGE_PNG, IMAGE_WEBP};
use zeroize::{Zeroize, Zeroizing};

use matrix_sdk::stream::StreamExt;

use crate::ui::icons::AppIcon;
use crate::ui::session::UserSession;
use crate::ui::states::input::AppInputStates;
use crate::ui::tasks::UserTodoTasks;
use crate::ui::views::dashboard::Dashboard;

pub const APP_ID: &str = "io.github.kir68k.met";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Default, PartialEq)]
pub enum AppState {
    Loading,
    #[default]
    Login,
    Dashboard,
}

/// Last received message event in a room
#[derive(Clone, Debug, PartialEq)]
pub enum LastMessage {
    Text(SharedString),
    Image,
    Video,
    Audio,
    File(SharedString), // filename
    Unknown,
    None,
}

impl LastMessage {
    pub fn display_text(&self) -> SharedString {
        match self {
            LastMessage::Text(text) => text.clone(),
            LastMessage::Image => "Sent an image".into(),
            LastMessage::Video => "Sent a video".into(),
            LastMessage::Audio => "Sent an audio message".into(),
            LastMessage::File(name) => format!("Sent a file: {}", name).into(),
            LastMessage::Unknown => "Sent a message".into(),
            LastMessage::None => "No messages".into(),
        }
    }
}

/// Holds UI-relevant room data to display as a card or on the sidebar
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomData {
    pub display_name: Option<SharedString>,
    pub avatar: Option<Arc<gpui::Image>>,
    pub last_msg: Option<LastMessage>,
}

impl RoomData {
    pub fn update(&mut self, new: Self) {
        self.display_name = new.display_name;
        self.avatar = new.avatar;
        self.last_msg = new.last_msg;
    }
}

/// Global rate limit counter
pub struct MediaRateLimit {
    /// The current count of rate limited requests since the last update/poll
    current: u32,
    /// The ceiling; functionality requiring rate limited requests should
    /// be exited before making a new request if this is hit
    ceiling: u32,
    /// Timeout before decrementing the rate limit since the last update/poll
    retry_ms: u32,
    /// Last time the counter was updated
    last_update: std::time::Instant,
}

impl Global for MediaRateLimit {}

impl MediaRateLimit {
    pub fn init(cx: &mut App) {
        cx.set_global(MediaRateLimit {
            current: 0,
            ceiling: 25,
            retry_ms: 2000,
            last_update: std::time::Instant::now(),
        });
    }

    /// Check the counter and increment by 1.
    /// Handles decreasing the counter based on time passed.
    ///
    /// This should be used right before making requests to rate-limited endpoints.
    // (admittedly i forgot what i was doing here)
    pub fn check_and_increment(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_update).as_millis();

        tracing::info!(
            "Current rate limits: {} of {} per {}ms as of {}s ago",
            self.current,
            self.ceiling,
            self.retry_ms,
            self.last_update.elapsed().as_secs()
        );

        if elapsed >= self.retry_ms as u128 {
            let decay = (elapsed / self.retry_ms as u128) as u32;
            self.current = self.current.saturating_sub(decay);
            // update by the decay
            self.last_update +=
                std::time::Duration::from_millis(decay as u64 * self.retry_ms as u64);
        }

        let can_request = if self.current < self.ceiling {
            tracing::info!("Request allowed; not rate limited");
            self.current += 1;
            true
        } else {
            tracing::info!("Request denied; ceiling hit");
            false
        };

        tracing::debug!(
            "Rate limit update: {} out of {}",
            self.current,
            self.ceiling
        );

        can_request
    }
}

impl RoomData {
    pub fn has_picture_avatar(&self) -> bool {
        self.avatar.is_some()
    }
}

#[derive(Clone, Default, PartialEq, IntoElement)]
pub enum SyncStatus {
    #[default]
    Unsynced,
    Syncing,
    Error(SharedString),
}

impl RenderOnce for SyncStatus {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (icon, text) = match self {
            SyncStatus::Unsynced => (AppIcon::SyncDisabled, "Unsynced"),
            SyncStatus::Syncing => (AppIcon::SyncEnabled, "Syncing"),
            SyncStatus::Error(_) => (AppIcon::SyncError, "Sync error"),
        };

        h_flex()
            .flex_shrink()
            .gap_2()
            .justify_between()
            .child(icon)
            .child(text)
    }
}

#[derive(Clone, Default, PartialEq, IntoElement)]
pub enum CrossStatus {
    #[default]
    Inactive,
    Active,
    Partial,
}

impl RenderOnce for CrossStatus {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (icon, text) = match self {
            CrossStatus::Inactive => (AppIcon::GeneralError, "Unverified"),
            CrossStatus::Active => (AppIcon::GeneralSuccess, "Verified"),
            CrossStatus::Partial => (AppIcon::GeneralPending, "Partially verified"),
        };

        h_flex()
            .flex_shrink()
            .gap_2()
            .justify_between()
            .child(icon)
            .child(text)
    }
}

pub struct ExportApp {
    pub version: &'static str,
    pub input_states: AppInputStates,
    pub user: ExportUser,
    pub state: AppState,
    pub dashboard_view: Option<Entity<Dashboard>>,
    pub room_data_cache: HashMap<ruma::OwnedRoomId, RoomData>,
    pub sync_status: SyncStatus,
    pub cross_status: CrossStatus,
    pub todo_tasks: UserTodoTasks,
}

impl Clone for ExportApp {
    fn clone(&self) -> Self {
        Self {
            version: self.version,
            input_states: self.input_states.clone(),
            user: self.user.clone(),
            state: self.state.clone(),
            dashboard_view: self.dashboard_view.clone(),
            room_data_cache: self.room_data_cache.clone(),
            sync_status: self.sync_status.clone(),
            cross_status: self.cross_status.clone(),
            // the task system works using a `Box<dyn TodoTaskBehavior>` so it can't be cloned
            // this doesn't really matter for cloning the root app tho
            todo_tasks: UserTodoTasks::new(),
        }
    }
}

impl EventEmitter<()> for ExportApp {}

#[derive(Clone, Debug, Default)]
pub struct ExportUser {
    userid: SharedString,
    password: Zeroizing<String>,
    data_dir: PathBuf,
    session_file: PathBuf,
    pub room_list: Vec<Room>,
    pub client: Option<Client>,
}

impl ExportUser {
    fn new() -> Self {
        let data_dir = dirs::data_dir()
            .expect("No app data directory found.")
            .join(APP_ID)
            .join("sessions");
        Self {
            data_dir: data_dir.clone(),
            session_file: data_dir.join("user.session"),
            ..Default::default()
        }
    }

    fn set_client(&mut self, client: Client) {
        self.client = Some(client);
    }

    /// Check if the session file exists
    fn has_session_file(&self) -> bool {
        self.session_file.exists()
    }
}

impl ExportApp {
    /// convenience method to check if we're actually logged in
    ///
    /// that means: having a client, and fulfilling `client.is_active()`
    pub fn is_logged_in(&self) -> bool {
        if let Some(client) = &self.user.client {
            client.is_active()
        } else {
            false
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            version: APP_VERSION,
            input_states: AppInputStates::new(window, cx),
            user: ExportUser::new(),
            state: AppState::Login,
            dashboard_view: None,
            room_data_cache: HashMap::default(),
            sync_status: SyncStatus::default(),
            cross_status: CrossStatus::default(),
            todo_tasks: UserTodoTasks::new(),
        }
    }

    pub fn get_room_data(&mut self, room: Room, cx: &mut Context<Self>) {
        let room_id = room.room_id().to_owned();

        let not_rate_limited = cx.global_mut::<MediaRateLimit>().check_and_increment();

        cx.spawn(move |view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // get the display name
                let display_name = room
                    .display_name()
                    .await
                    .ok()
                    .map(|name| name.to_string().into());

                // Thumbnail = less memory (relatively anyway), less unneeded network I/O
                // also renders much nicer, downscaling a full file in the app makes it veeeeeery aliased >_<
                let avatar = if not_rate_limited {
                    tracing::debug!(
                        "Not rate limited, downloading {:?} ({:?})",
                        &display_name,
                        &room_id
                    );

                    // FIXME: i don't know why but *some* rooms simply refuse to return an avatar
                    room.avatar(matrix_sdk::media::MediaFormat::Thumbnail(
                        MediaThumbnailSettings::with_method(ruma::media::Method::Crop, 96u32.into(), 96u32.into()),
                    ))
                    .await?
                    .or_else(|| {
                        tracing::error!(
                            "No avatar found for {:?} ({:?}) | Return type None, should be Some(Vec<u8>)",
                            &display_name,
                            &room_id
                        );
                        None
                    })
                    .and_then(|bytes| match mimetype_detector::detect(&bytes).mime() {
                        IMAGE_PNG => Some((ImageFormat::Png, bytes)),
                        IMAGE_JPEG => Some((ImageFormat::Jpeg, bytes)),
                        IMAGE_WEBP => Some((ImageFormat::Webp, bytes)),
                        IMAGE_GIF => Some((ImageFormat::Gif, bytes)),
                        IMAGE_APNG => Some((ImageFormat::Bmp, bytes)),
                        mimetype => {
                            tracing::error!(
                                "Valid MIME types: {IMAGE_PNG}, {IMAGE_JPEG}, {IMAGE_WEBP}, {IMAGE_GIF}, {IMAGE_APNG} | Received invalid type: {mimetype}"
                            );
                            None
                        }
                    })
                    .map(|data| Image::from_bytes(data.0, data.1))
                    .map(Arc::new)
                } else {
                    None
                };

                // get and parse last message
                // FIXME (#17): Apparently this needs sliding sync, which I can't test right now.
                // Not sure if it explains the missing avatars on *some* rooms, but it'd explain the default "No messages".
                let last_event = room.latest_event();
                let last_msg = last_event.and_then(|event| {
                    use matrix_sdk::ruma::events::{
                        AnySyncMessageLikeEvent, AnySyncTimelineEvent,
                        room::message::{MessageType, SyncRoomMessageEvent},
                    };

                    // only handle stuff that can be displayed in ui
                    // that will correspond only to what LastMessage can take in
                    // it might grow in the future
                    match event.event().raw().deserialize().ok()? {
                        AnySyncTimelineEvent::MessageLike(
                            AnySyncMessageLikeEvent::RoomMessage(SyncRoomMessageEvent::Original(
                                msg,
                            )),
                        ) => match &msg.content.msgtype {
                            MessageType::Text(t) => Some(LastMessage::Text(t.body.clone().into())),
                            MessageType::Image(_) => Some(LastMessage::Image),
                            MessageType::Video(_) => Some(LastMessage::Video),
                            MessageType::Audio(_) => Some(LastMessage::Audio),
                            MessageType::File(f) => Some(LastMessage::File(f.body.clone().into())),
                            _ => Some(LastMessage::Unknown),
                        },
                        AnySyncTimelineEvent::MessageLike(_) => Some(LastMessage::Unknown),
                        _ => None,
                    }
                });

                // update cache and re-render
                view.update(&mut cx, |app: &mut Self, cx: &mut Context<Self>| {
                    let new = RoomData {
                        display_name,
                        avatar,
                        last_msg: Some(last_msg.unwrap_or(LastMessage::None)),
                    };

                    // if the entry exists, only update the avatar if we got one
                    // if it doesn't, insert everything
                    app.room_data_cache
                        .entry(room_id)
                        .and_modify(|data| data.update(new.clone()))
                        .or_insert(new);

                    cx.notify();
                })?;

                anyhow::Ok(())
            }
        })
        .detach();
    }

    // FIXME: the rate limit has nothing to do with this (maybe)
    // this might be cus no verification + dendrite is stupid with old events
    // finish the dashboard main panel put to-do list for users there
    // e.g.:
    // - Import room keys
    //   - [smol text] Import room keys to bootstrap decryption.
    // - Verify yourself
    //   - [smol text] Cross-signing retrieve events.
    /// Add the default user todo tasks
    pub fn init_todo_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.user.client.clone() else {
            return;
        };

        cx.spawn(|view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // FIXME: Don't display this if keys were already imported
                // checking that would require putting it in the session file
                // then reading it here after deserializing.
                view.update(&mut cx, |this, _| {
                    if this.todo_tasks.is_empty() {
                        this.todo_tasks
                            .add_task(tasks::key_import::InitialKeyImport);
                    }
                })?;

                if let Some(cross) = client.encryption().cross_signing_status().await
                    && !(cross.is_complete()
                        || cross.has_master
                        || cross.has_self_signing
                        || cross.has_user_signing)
                {
                    view.update(&mut cx, |this, _| {
                        this.todo_tasks
                            .add_task(tasks::verification::VerificationTask);
                    })?;
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    /// Try to download the avatar/last msg of all rooms (updates RoomData)
    pub fn update_all_rooms(&mut self, force_update: bool, cx: &mut Context<Self>) {
        let rooms = self.user.room_list.clone();

        for room in rooms {
            if let Some(cached) = self.room_data_cache.get(room.room_id())
                && cached.has_picture_avatar()
                && !force_update
            {
                return;
            } else {
                self.get_room_data(room, cx);
            }
        }
    }

    /// Attempt to restore a saved session
    pub fn restore_session(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let user = self.user.clone();

        cx.spawn(|view: WeakEntity<ExportApp>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                match user.restore_session().await {
                    Ok((client, userid)) => {
                        let rooms = client.joined_rooms();

                        view.update(
                            &mut cx,
                            |app: &mut ExportApp, cx: &mut Context<ExportApp>| {
                                app.user.set_client(client);
                                app.user.userid = userid.into();
                                app.user.room_list = rooms;
                                app.state = AppState::Dashboard;

                                app.init_todo_tasks(cx);
                                app.background_sync(cx);
                                cx.notify();
                            },
                        )?;
                    }
                    Err(e) => {
                        eprintln!("Session restoration failed: {}", e);

                        view.update(
                            &mut cx,
                            |app: &mut ExportApp, cx: &mut Context<ExportApp>| {
                                app.state = AppState::Login;
                                app.dashboard_view = None;
                                cx.notify();
                            },
                        )?;
                    }
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    pub fn login(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let user = self.user.clone();

        cx.spawn(|view: WeakEntity<ExportApp>, cx: &mut AsyncApp| {
            // AsyncApp is a weak pointer, cloning is fine.
            // cloned as the future outlives this closure.
            let mut cx = cx.clone();
            async move {
                match user.login().await {
                    Ok((client, _)) => {
                        let rooms = client.joined_rooms();

                        view.update(
                            &mut cx,
                            |app: &mut ExportApp, cx: &mut Context<ExportApp>| {
                                app.user.set_client(client);
                                app.user.room_list = rooms;
                                app.user.password.zeroize();
                                app.state = AppState::Dashboard;

                                app.init_todo_tasks(cx);
                                app.background_sync(cx);
                                cx.notify();
                            },
                        )?;
                    }
                    Err(e) => {
                        eprintln!("Login failed: {}", e);

                        view.update(
                            &mut cx,
                            |_app: &mut ExportApp, cx: &mut Context<ExportApp>| {
                                // stay in login state on failure
                                cx.notify();
                            },
                        )?;
                    }
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    pub fn logout(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let user = self.user.clone();

        cx.spawn(|view: WeakEntity<ExportApp>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                // this will rm the session file too.
                if user.client.is_some() {
                    match user.logout().await {
                        Ok(_) => {
                            view.update(&mut cx, |app, cx| {
                                app.user.client = None;
                                app.state = AppState::Login;
                                app.dashboard_view = None;
                                cx.notify();
                            })?;
                        }
                        Err(e) => {
                            eprintln!("Failed to log out: {}", e);
                        }
                    }
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    fn background_sync(&mut self, cx: &mut Context<Self>) {
        // don't spawn this twice
        if self.sync_status == SyncStatus::Syncing {
            return;
        }

        let Some(client) = self.user.client.clone() else {
            return;
        };

        self.sync_status = SyncStatus::Syncing;
        cx.notify();

        cx.spawn(|view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let mut sync =
                    Box::pin(
                        client
                            .sync_stream(SyncSettings::default().set_presence(
                                matrix_sdk::ruma::presence::PresenceState::Unavailable,
                            ))
                            .await,
                    );

                while let Some(sync_response) = sync.next().await {
                    // check if the client exists, exit if no
                    // avoids log spam and having a useless task :p
                    if let Ok(false) = view.read_with(&cx, |view, _| view.user.client.is_some()) {
                        return Err(anyhow::anyhow!("No client, stopping background sync."));
                    }

                    match sync_response {
                        Ok(_) => {
                            let rooms = client.joined_rooms();
                            view.update(&mut cx, |app, cx| {
                                app.user.room_list = rooms;
                                if app.sync_status != SyncStatus::Syncing {
                                    app.sync_status = SyncStatus::Syncing;
                                }
                                cx.notify();
                            })?;
                        }
                        Err(e) => {
                            let err = SharedString::from(e.to_string());
                            view.update(&mut cx, |app, cx| {
                                app.sync_status = SyncStatus::Error(err);
                                cx.notify();
                            })?;
                        }
                    }
                }

                anyhow::Ok(())
            }
        })
        .detach();

        // spawned alongside the main sync task, every 30 seconds, this updates:
        // - all room data (see the RoomData type)
        // - cross-signing status
        cx.spawn(|view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let mut roomdata_interval =
                    async_time::interval(async_time::Duration::from_secs(30));
                roomdata_interval.set_missed_tick_behavior(async_time::MissedTickBehavior::Skip);

                loop {
                    roomdata_interval.tick().await;

                    let Some(client) = view.read_with(&cx, |view, _| view.user.client.clone())?
                    else {
                        break;
                    };

                    if let Some(upd) = client.encryption().cross_signing_status().await {
                        view.update(&mut cx, |this, cx| {
                            if upd.is_complete() {
                                this.cross_status = CrossStatus::Active;
                            } else if upd.has_master || upd.has_self_signing || upd.has_user_signing
                            {
                                this.cross_status = CrossStatus::Partial;
                            } else {
                                this.cross_status = CrossStatus::Inactive;
                            }
                            cx.notify();
                        })?;
                    }

                    view.update(&mut cx, |this, cx| {
                        this.update_all_rooms(true, cx);
                        cx.notify();
                    })?;
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    fn render_loading(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(16.0))
                    .text_color(cx.theme().foreground)
                    .child("Loading session..."),
            )
            .into_any_element()
    }
}
