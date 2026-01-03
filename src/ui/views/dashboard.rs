use std::collections::HashMap;

use gpui::{prelude::FluentBuilder, *};
use matrix_sdk::{Room, ruma};

use gpui_component::{
    ActiveTheme, Sizable,
    avatar::Avatar,
    button::Button,
    h_flex,
    list::{List, ListDelegate, ListItem, ListState},
    v_flex,
};

use crate::ui::{ExportApp, icons::AppIcon};

pub struct RoomListDelegate {
    app: WeakEntity<ExportApp>,
    dashboard: WeakEntity<Dashboard>,
}

impl ListDelegate for RoomListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, cx: &App) -> usize {
        self.app
            .read_with(cx, |app, _| app.user.room_list.len())
            .unwrap_or_else(|_| 0)
    }

    fn render_item(
        &mut self,
        ix: gpui_component::IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        self.app
            .read_with(cx, |app, cx| {
                let room = app.user.room_list.get(ix.row)?;
                Some(ListItem::new(ix).child(Dashboard::render_room_item(
                    room,
                    &app.room_data_cache,
                    cx,
                )))
            })
            .ok()
            .flatten()
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<gpui_component::IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        todo!();
        // if let Some(ix) = ix
        //     && let Some(app) = self.app.upgrade()
        //     && let Some(dashboard) = self.dashboard.upgrade()
        // {
        //     let Some(room) = app.read_with(cx, |state, _| {
        //         Some(state.user.room_list.get(ix.row)?.clone())
        //     }) else {
        //         return;
        //     };
        //
        //     dashboard.update(cx, |dashboard, cx| {
        //         dashboard.show_room_view(room.room_id().to_owned(), cx);
        //     });
        // }
    }
}

pub struct Dashboard {
    app: WeakEntity<ExportApp>,
    room_list: Entity<ListState<RoomListDelegate>>,
    main_view: AnyView,
}

impl Dashboard {
    pub fn new(app: WeakEntity<ExportApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = RoomListDelegate {
            app: app.clone(),
            dashboard: cx.entity().downgrade(),
        };
        let room_list = cx.new(|cx| ListState::new(delegate, window, cx));
        let main_view: AnyView = cx.new(|cx| TodoView::new(app.clone(), window, cx)).into();

        Self {
            app,
            room_list,
            main_view,
        }
    }

    /// Switch the main panel to a different view
    pub fn set_main_view(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.main_view = view;
        cx.notify();
    }

    // pub fn show_room_view(&mut self, room_id: ruma::OwnedRoomId, cx: &mut Context<Self>) {
    //     let app = self.app.clone();
    //     let view: AnyView = cx.new(|cx| RoomView::new(app, room_id, cx)).into();
    //     self.set_main_view(view, cx);
    // }

    fn render_room_item(
        room: &Room,
        cache: &HashMap<ruma::OwnedRoomId, crate::ui::RoomData>,
        cx: &App,
    ) -> AnyElement {
        let room_id = room.room_id();
        let cached = cache.get(room_id);

        let display_name = cached
            .and_then(|d| d.display_name.clone())
            .unwrap_or_else(|| room_id.to_string().into());

        let last_msg = cached
            .and_then(|d| d.last_msg.as_ref().map(|m| m.display_text()))
            .unwrap_or_else(|| "Loading...".into());

        let avatar_fallback = display_name
            .chars()
            .next()
            .unwrap_or_else(|| '?')
            .to_uppercase()
            .to_string();

        h_flex()
            .justify_around()
            .items_center()
            .text_center()
            .gap_3()
            .p_2()
            .child(
                Avatar::new()
                    .when_some(cached.and_then(|d| d.avatar.clone()), |this, src| {
                        this.src(ImageSource::from(src))
                    })
                    .name(avatar_fallback)
                    .with_size(px(40.0)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(display_name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_nowrap()
                            .child(last_msg),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .child(
                v_flex()
                    .min_w(rems(14.0))
                    .h_full()
                    .bg(cx.theme().sidebar)
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(div().p_4().child("Rooms"))
                    .child(
                        div()
                            .flex_1()
                            .child(List::new(&self.room_list).scrollbar_visible(false)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(self.main_view.clone()),
            )
    }
}

pub struct TodoView {
    app: WeakEntity<ExportApp>,
    selected_ix: Option<usize>,
    task_view: Option<AnyView>,
}

impl TodoView {
    /// Go back to the task list from a task view
    pub fn back_to_list(&mut self, cx: &mut Context<Self>) {
        self.selected_ix = None;
        self.task_view = None;
        cx.notify();
    }
}

impl TodoView {
    pub fn new(app: WeakEntity<ExportApp>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            app,
            selected_ix: None,
            task_view: None,
        }
    }

    pub fn select_task(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app_entity) = self.app.upgrade() else {
            return;
        };

        let app_weak = self.app.clone();

        let task_view = app_entity.update(cx, |app, cx| {
            app.todo_tasks
                .get(ix)
                .map(|task| task.create_view(app_weak.clone(), window, cx))
        });

        self.selected_ix = Some(ix);
        self.task_view = task_view;
        cx.notify();
    }
}

impl Render for TodoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view_entity = cx.entity().clone();

        // this should be a .when() instead?
        if let Some(task_view) = &self.task_view {
            return v_flex()
                .size_full()
                .child(
                    // Back button header
                    h_flex()
                        .p_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(Button::new("back_to_tasks").label("Back").on_click({
                            let view = view_entity.clone();
                            move |_, _window, cx| {
                                view.update(cx, |view, cx| {
                                    view.back_to_list(cx);
                                });
                            }
                        })),
                )
                .child(div().flex_1().overflow_hidden().child(task_view.clone()))
                .into_any_element();
        }

        let tasks: Vec<_> = self
            .app
            .read_with(cx, |app, app_cx| {
                app.todo_tasks
                    .tasks()
                    .iter()
                    .enumerate()
                    .map(|(i, task)| {
                        let is_finished = task.is_finished(app, app_cx);
                        (i, task.title(), task.label(), is_finished)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let border_color = cx.theme().border;
        let muted_color = cx.theme().muted;
        let muted_fg = cx.theme().muted_foreground;
        let accent_hover = cx.theme().accent.opacity(0.1);

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // Header
            .child(
                // i was thinking of using gpui_component::Label but `secondary` doesn't make a line break :I
                // i might make my own version later, idk.
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child("Getting Started"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child("Filling these out is currently required."),
                    ),
            )
            // Grid thing for tasks
            .child(
                h_flex().flex_wrap().gap_4().children(tasks.into_iter().map(
                    move |(i, title, label, is_finished)| {
                        let view = view_entity.clone();

                        div()
                            .id(ElementId::Name(format!("task-{}", i).into()))
                            .w(rems(20.0))
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .when(is_finished, |this| {
                                this.opacity(0.5).cursor_default().border_color(muted_color)
                            })
                            .when(!is_finished, |this| {
                                this.cursor_pointer()
                                    .border_color(border_color)
                                    .hover(|s| s.bg(accent_hover).border_color(cx.theme().accent))
                                    .on_click(move |_, window, cx| {
                                        view.update(cx, |view, cx| {
                                            view.select_task(i, window, cx);
                                        });
                                    })
                            })
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(title),
                                    )
                                    .child(div().text_sm().text_color(muted_fg).child(label))
                                    .when(is_finished, |this| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().success)
                                                .child(AppIcon::GeneralSuccess)
                                                .child("Completed"),
                                        )
                                    }),
                            )
                    },
                )),
            )
            .into_any_element()
    }
}
