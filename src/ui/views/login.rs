use gpui::*;
use gpui_component::{ActiveTheme as _, button::Button, input::Input};

use crate::ui::ExportApp;

impl ExportApp {
    pub fn render_login(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(32.0))
                    .gap(px(32.0))
                    // Header
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(24.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(cx.theme().foreground)
                                    .child("met."),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("[wip] Archiving utility for matrix"),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .text_size(px(12.0))
                                    .child(self.version),
                            ),
                    )
                    // login input
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.0))
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().foreground)
                                    .child("Login"),
                            )
                            .child(
                                div()
                                    .w_1_3()
                                    .child(
                                        Input::new(&self.input_states.login.username)
                                            .cleanable(true),
                                    )
                                    .child(
                                        Input::new(&self.input_states.login.password)
                                            .mask_toggle()
                                            .cleanable(true),
                                    ),
                            )
                            .child(
                                Button::new("login_button")
                                    .w_1_3()
                                    .label("Log in")
                                    .on_click(cx.listener(|view, _, window: &mut Window, cx| {
                                        view.login(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
