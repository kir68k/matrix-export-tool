use gpui::{IntoElement, RenderOnce};
use gpui_component::{Icon, IconNamed};

#[derive(IntoElement)]
pub enum AppIcon {
    SyncDisabled,
    SyncEnabled,
    SyncError,
    GeneralSuccess,
    GeneralError,
    GeneralPending,
}

impl IconNamed for AppIcon {
    fn path(self) -> gpui::SharedString {
        match self {
            AppIcon::SyncDisabled => "icons/refresh-cw-off.svg",
            AppIcon::SyncEnabled => "icons/refresh-cw.svg",
            AppIcon::SyncError => "icons/server-crash.svg",
            AppIcon::GeneralSuccess => "icons/circle-check.svg",
            AppIcon::GeneralError => "icons/circle-x.svg",
            AppIcon::GeneralPending => "icons/circle-ellipsis.svg",
        }
        .into()
    }
}

impl RenderOnce for AppIcon {
    fn render(self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> impl gpui::IntoElement {
        Icon::empty().path(self.path())
    }
}
