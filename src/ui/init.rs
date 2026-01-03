use std::borrow::Cow;
use std::path::PathBuf;

use anyhow::Context as _;
use gpui::*;
use gpui_component::{Root, Theme, ThemeMode, TitleBar, v_flex};
use gpui_component::{TITLE_BAR_HEIGHT, ThemeRegistry};
use rust_embed::RustEmbed;

use crate::ui::AppState;
use crate::ui::ExportApp;
use crate::ui::titlebar::AppTitleBar;
use crate::ui::views::dashboard::Dashboard;
use crate::ui::{APP_ID, MediaRateLimit};

pub struct AppRoot {
    focus_handle: FocusHandle,
    title_bar: Entity<AppTitleBar>,
    app: Entity<ExportApp>,
    view: AnyView,
    last_state: Option<AppState>,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app = cx.new(|cx| {
            let mut app = ExportApp::new(window, cx);
            if app.user.has_session_file() {
                app.state = AppState::Loading;
                app.restore_session(window, cx);
            }
            app
        });

        MediaRateLimit::init(cx);

        let title_bar = cx.new(|cx| AppTitleBar::new("met", window, cx));
        let view = Self::create_view(&app, window, cx);

        // subscribe to app changes
        // (i don't think this does a lot right now)
        cx.subscribe(&app, |_, _, _: &(), cx| {
            cx.notify();
        })
        .detach();

        let last_state = Some(app.read(cx).state.clone());

        Self {
            focus_handle: cx.focus_handle(),
            title_bar,
            app,
            view,
            last_state,
        }
    }

    fn create_view(
        app: &Entity<ExportApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyView {
        let app_state = app.read(cx).state.clone();
        match app_state {
            AppState::Dashboard => {
                let app_weak = app.downgrade();
                cx.new(|cx| Dashboard::new(app_weak, window, cx)).into()
            }
            _ => {
                let app_weak = app.downgrade();
                cx.new(|cx| StaticView::new(app_weak, window, cx)).into()
            }
        }
    }

    fn update_view(&mut self, state: AppState, window: &mut Window, cx: &mut Context<Self>) {
        let app_weak = self.app.downgrade();
        // Only recreate the view if the state necessitates a different view type
        // or we need to refresh the dashboard instance.
        self.view = match state {
            AppState::Dashboard => cx.new(|cx| Dashboard::new(app_weak, window, cx)).into(),
            _ => cx.new(|cx| StaticView::new(app_weak, window, cx)).into(),
        };
    }
}

/// Helper object for old ""views"".
/// Login/loading were made before the main dashboard view, and were
/// simply `render_[x]` functions returning an element
///
/// (initially i put most data into ExportApp, also AppRoot didn't exist)
/// This note and type should be removed before merging.
struct StaticView {
    app: WeakEntity<ExportApp>,
}

impl StaticView {
    fn new(app: WeakEntity<ExportApp>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { app }
    }
}

impl Render for StaticView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.app
            .update(cx, |app, cx| match app.state {
                AppState::Loading => app.render_loading(window, cx),
                AppState::Login => app.render_login(window, cx),
                AppState::Dashboard => div().into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Focusable for AppRoot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        // Sync and cross status get put into the title bar
        let (app_state, sync_status, cross_status) = self.app.read_with(cx, |app, _| {
            (
                app.state.clone(),
                app.sync_status.clone(),
                app.cross_status.clone(),
            )
        });

        if Some(&app_state) != self.last_state.as_ref() {
            self.update_view(app_state.clone(), window, cx);
            self.last_state = Some(app_state);
        }

        div()
            .id("app-root")
            .size_full()
            .relative()
            .child(
                v_flex()
                    .size_full()
                    .child(self.title_bar.clone())
                    .child(
                        div()
                            .track_focus(&self.focus_handle)
                            .flex_1()
                            .overflow_hidden()
                            .child(self.view.clone()),
                    )
                    .children(sheet_layer)
                    .children(dialog_layer)
                    .children(notification_layer),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(TITLE_BAR_HEIGHT)
                    .flex()
                    .text_sm()
                    .items_center()
                    .justify_center()
                    .child(sync_status)
                    .child(div().text_base().child(" | "))
                    .child(cross_status),
            )
    }
}

#[derive(RustEmbed)]
#[folder = "./assets"]
#[include = "icons/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Self::get(path)
            .map(|f| Some(f.data))
            .with_context(|| format!("loading asset at path {path:?}"))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|p| p.starts_with(path).then(|| p.into()))
            .collect())
    }
}

pub fn create_new_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1024.0), px(640.0)), cx);

    cx.spawn(async move |cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                window_min_size: Some(size(px(960.0), px(600.0))),
                kind: WindowKind::Normal,
                app_id: Some(APP_ID.into()),
                #[cfg(target_os = "linux")]
                window_background: gpui::WindowBackgroundAppearance::Blurred,
                #[cfg(target_os = "linux")]
                window_decorations: Some(gpui::WindowDecorations::Client),
                ..Default::default()
            },
            |window, cx| {
                Theme::change(ThemeMode::Dark, Some(window), cx);

                let theme_name = SharedString::from("Shadcn Dark");

                if let Err(err) =
                    ThemeRegistry::watch_dir(PathBuf::from("./themes"), cx, move |cx| {
                        if let Some(theme) =
                            ThemeRegistry::global(cx).themes().get(&theme_name).cloned()
                        {
                            Theme::global_mut(cx).apply_config(&theme);
                        }
                    })
                {
                    tracing::error!("Failed to load themes: {err}");
                }

                let root_view = cx.new(|cx| AppRoot::new(window, cx));
                cx.new(|cx| Root::new(root_view, window, cx))
            },
        )?;
        Ok::<_, anyhow::Error>(())
    })
    .detach();
}

/// Helper fn to initialize things before running the UI.
pub fn init() -> anyhow::Result<()> {
    // TODO: Decide whether tokio should be a dependency
    // It's the matrix sdk's executor, so this is just a re-export,
    // and while I might integrate exporting into gpui tasks,
    // non-blocking file i/o will require either tokio or parts of smol.
    let runtime = matrix_sdk::executor::Runtime::new()?;
    let _guard = runtime.enter();

    // TODO: Works on Linux, but does it on other platforms?
    // Should be tested *somehow* partly due to being an unstable feature.
    cfg_select! {
        target_os = "linux" => {
            keyring_core::set_default_store(zbus_secret_service_keyring_store::Store::new()?);
        }
        target_os = "windows" => {
            keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);
        }
        target_os = "macos" => {
            keyring_core::set_default_store(apple_native_keyring_store::protected::Store::new()?);
        }
    }

    // Build the gpui application which will be running
    // At some point in the future, the HTTP client might be made here,
    // and added by `with_http_client`, then later added to matrix-sdk's `Client`.
    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        create_new_window(cx);
    });

    Ok(())
}
