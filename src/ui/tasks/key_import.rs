use gpui::{
    AnyView, App, AppContext, AsyncApp, Context, Entity, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, Styled, WeakEntity, Window, prelude::FluentBuilder,
};
use gpui_component::{
    ActiveTheme, Disableable, WindowExt,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    v_flex,
};
use std::path::PathBuf;

use crate::ui::{ExportApp, tasks::TodoTaskBehavior};

pub struct InitialKeyImport;

impl TodoTaskBehavior for InitialKeyImport {
    fn id(&self) -> &'static str {
        "initial_key_import"
    }

    fn title(&self) -> SharedString {
        "Import your room keys".into()
    }

    fn label(&self) -> SharedString {
        "Make an initial import to bootstrap decryption.".into()
    }

    fn create_view(
        &self,
        app: WeakEntity<ExportApp>,
        window: &mut Window,
        cx: &mut Context<ExportApp>,
    ) -> AnyView {
        let view: Entity<KeyImportView> = cx.new(|cx| KeyImportView::new(app, window, cx));
        // at least rn we have to return *something* even tho dialogs don't do that.
        // KeyImportView itself doesn't do much.
        let return_view = view.clone();

        let password = view.read(cx).password.clone();

        // make a window dialog
        // I wanted to use this instead of making a whole view inside the dashboard,
        // as importing keys is relatively simple.
        // This *might* be reverted at some point, it'd be more consistent... >_>
        window.open_dialog(cx, move |dialog, _, cx: &mut App| {
            dialog
                .title("Initial key import")
                .overlay(true)
                .keyboard(false)
                .close_button(true)
                .overlay_closable(false)
                .child(
                    v_flex().gap_3().child(Input::new(&password)).child(
                        Button::new("key_file_picker")
                            .primary()
                            .label("Select file")
                            .when(!view.read(cx).paths.is_empty(), |btn| btn.disabled(true))
                            .on_click({
                                let view = view.clone();
                                move |_, _window, cx: &mut App| {
                                    // This returns a `futures::oneshot::Receiver`
                                    // the sync app context `cx: &mut App` supports this method
                                    // but `AsyncApp`, which we get inside `cx.spawn`, does not.
                                    // Running `try_recv()` won't work here.
                                    let paths = cx.prompt_for_paths(PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: false,
                                        prompt: None,
                                    });

                                    cx.spawn({
                                        let view = view.clone();
                                        |cx: &mut AsyncApp| {
                                            let mut cx = cx.clone();
                                            async move {
                                                if let Ok(Ok(paths)) = paths.await {
                                                    view.update(&mut cx, |this, cx| {
                                                        tracing::info!(
                                                            "Selected file: {:?}",
                                                            paths
                                                        );
                                                        this.paths = paths.unwrap_or_default();
                                                        cx.notify();
                                                    })
                                                    .ok();
                                                }
                                            }
                                        }
                                    })
                                    .detach();
                                }
                            }),
                    ),
                )
                .footer({
                    let view = view.clone();
                    move |_, _, _, _cx| {
                        vec![
                            Button::new("confirm").primary().label("Import").on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    tracing::info!("Importing keys");
                                    view.update(cx, |this, cx| {
                                        this.import_keys(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                            Button::new("cancel").outline().label("Cancel").on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.reset(window, cx);
                                    });
                                    window.close_dialog(cx);
                                }
                            }),
                        ]
                    }
                })
        });

        return_view.into()
    }
}

/// View and data for the initial key import task
/// NOTE: This is *currently* mainly for task data and functionality,
/// as it should be isolated from [`ExportApp`] itself (for clarity).
/// The rendered view does nothing, as this task is a dialog.
#[derive(Debug)]
pub struct KeyImportView {
    app: WeakEntity<ExportApp>,
    paths: Vec<PathBuf>,
    password: Entity<InputState>,
    success: bool,
    error: Option<SharedString>,
}

impl KeyImportView {
    pub fn new(app: WeakEntity<ExportApp>, window: &mut Window, cx: &mut App) -> Self {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Key passphrase")
                .masked(true)
        });

        Self {
            app,
            paths: Vec::new(),
            password,
            success: false,
            error: None,
        }
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut App) {
        self.paths.clear();
        self.password.update(cx, |this, cx| {
            this.replace("", window, cx);
        });
        self.success = false;
        self.error = None;
    }

    pub fn import_keys(&self, cx: &mut Context<Self>) {
        tracing::debug!("Values of KeyImportView: {:#?}", self);

        if self.paths.is_empty() || self.password.read(cx).value().is_empty() {
            return;
        }

        let Some(path) = self.paths.first().cloned() else {
            return;
        };
        let Ok(Some(client)) = self.app.read_with(cx, |app, _| app.user.client.clone()) else {
            return;
        };
        let passphrase = self.password.read(cx).value();

        cx.spawn(|view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let result = client
                    .encryption()
                    .import_room_keys(path, &passphrase)
                    .await;

                view.update(&mut cx, |this: &mut Self, cx: &mut Context<Self>| {
                    match result {
                        Ok(_res) => {
                            this.success = true;
                            this.app
                                .update(cx, |app, _| {
                                    app.todo_tasks.mark_completed("initial_key_import");
                                })
                                .ok();
                        }
                        Err(e) => {
                            this.success = false;
                            this.error = Some(format!("Error: {}", e).into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}

impl Render for KeyImportView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Dialog is opened in create_view, this just shows a placeholder
        v_flex().size_full().items_center().justify_center().child(
            gpui::div()
                .text_color(cx.theme().muted_foreground)
                .child("Key import dialog is open."),
        )
    }
}
