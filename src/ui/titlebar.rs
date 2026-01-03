use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::{TitleBar, h_flex};

pub struct AppTitleBar {
    title: SharedString,
    // app_menu_bar: Entity<AppMenuBar>,
    child: Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>,
    _subscriptions: Vec<Subscription>,
}

impl AppTitleBar {
    pub fn new(
        title: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // let app_menu_bar = AppMenuBar::new(window, cx);

        Self {
            title: title.into(),
            // app_menu_bar,
            child: Rc::new(|_, _| div().into_any_element()),
            _subscriptions: vec![],
        }
    }

    pub fn child<F, E>(mut self, f: F) -> Self
    where
        E: IntoElement,
        F: Fn(&mut Window, &mut App) -> E + 'static,
    {
        self.child = Rc::new(move |window, cx| f(window, cx).into_any_element());
        self
    }
}

impl Render for AppTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .child(h_flex().items_center().child(self.title.clone()))
            .child(
                h_flex()
                    .items_center()
                    .px_2()
                    .gap_2()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child((self.child.clone())(window, cx)), // .child(
                                                              //     div().relative().child(
                                                              //         Badge::new()
                                                              //             .when_else(
                                                              //                 cx.theme().is_dark(),
                                                              //                 |badge| badge.icon(IconName::Moon),
                                                              //                 |badge| badge.icon(IconName::Sun),
                                                              //             )
                                                              //             .child(
                                                              //                 Button::new("theme_switch")
                                                              //                     .small()
                                                              //                     .ghost()
                                                              //                     .compact()
                                                              //                     .on_click(|_, _, cx| {
                                                              //                         set_theme(cx);
                                                              //                     }),
                                                              //             ),
                                                              //     ),
                                                              // ),
            )
    }
}

mod menubar {}
