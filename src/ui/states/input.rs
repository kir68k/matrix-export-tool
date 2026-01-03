use gpui::{AppContext, Context, Entity, SharedString, Window};
use gpui_component::input::InputState;

use crate::ui::ExportApp;

#[derive(Clone)]
pub struct AppInputStates {
    pub login: LoginInputs,
}

#[derive(Clone)]
pub struct LoginInputs {
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
}

impl AppInputStates {
    pub fn new(window: &mut Window, cx: &mut Context<ExportApp>) -> Self {
        Self {
            login: LoginInputs::new(window, cx),
        }
    }
}

impl LoginInputs {
    pub fn new(window: &mut Window, cx: &mut Context<ExportApp>) -> Self {
        Self {
            username: cx.new(|cx| InputState::new(window, cx).placeholder("@user:example.org")),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Password")
                    .masked(true)
            }),
        }
    }
}
