use anyhow::Context as _;
use gpui::{
    AnyElement, AnyView, AppContext, AsyncApp, Context, FontWeight, IntoElement, ParentElement,
    Render, SharedString, Styled, WeakEntity, Window, rems,
};
use gpui_component::{
    ActiveTheme,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use matrix_sdk::{
    Client,
    encryption::verification::{
        CancelInfo, Emoji, SasState, SasVerification, VerificationRequest, VerificationRequestState,
    },
    ruma::events::key::verification::VerificationMethod,
    stream::StreamExt,
};

use crate::ui::{ExportApp, icons::AppIcon, tasks::TodoTaskBehavior};

const VERIFICATION_METHODS: &[VerificationMethod] = &[VerificationMethod::SasV1];

pub struct VerificationTask;

impl TodoTaskBehavior for VerificationTask {
    fn id(&self) -> &'static str {
        "verification"
    }

    fn title(&self) -> SharedString {
        "Verify the session".into()
    }

    fn label(&self) -> SharedString {
        "Enables cross-signing support.".into()
    }

    fn create_view(
        &self,
        app: WeakEntity<ExportApp>,
        _window: &mut Window,
        cx: &mut Context<ExportApp>,
    ) -> AnyView {
        cx.new(|_| VerificationView::new(app)).into()
    }
}

/// Custom object for the whole verification process.
pub enum VerificationFlowState {
    Idle,
    Requesting {
        request: VerificationRequest,
    },
    Ready {
        request: VerificationRequest,
        their_methods: Vec<VerificationMethod>,
    },
    // TODO: Qr(QrFlowState) >_>
    Sas(SasFlowState),
    Completed,
    Cancelled(CancelInfo),
}

/// sas-specific data.
pub enum SasFlowState {
    Started {
        sas: SasVerification,
    },
    KeysExchanged {
        sas: SasVerification,
        emojis: [Emoji; 7],
    },
    Confirming {
        sas: SasVerification,
    },
}

pub struct VerificationView {
    app: WeakEntity<ExportApp>,
    client: Option<Client>,
    state: VerificationFlowState,
}

impl VerificationView {
    pub fn new(app: WeakEntity<ExportApp>) -> Self {
        Self {
            app,
            client: None,
            state: VerificationFlowState::Idle,
        }
    }

    // this was mainly taken from src/utils/client.rs (CLI) and modified to integrate with gpui.
    fn start_verification(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };

        cx.spawn(|view: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let identity = client
                    .encryption()
                    .get_user_identity(client.user_id().unwrap())
                    .await?
                    .context("No user identity.")?;

                let request = identity
                    .request_verification_with_methods(VERIFICATION_METHODS.to_vec())
                    .await?;

                view.update(&mut cx, |this, cx| {
                    this.state = VerificationFlowState::Requesting {
                        request: request.clone(),
                    };
                    cx.notify();
                })?;

                while let Some(state) = request.changes().next().await {
                    match &state {
                        VerificationRequestState::Ready { their_methods, .. } => {
                            let their_methods = their_methods.clone();
                            view.update(&mut cx, |this, cx| {
                                this.state = VerificationFlowState::Ready {
                                    request: request.clone(),
                                    their_methods,
                                };
                                cx.notify();
                            })?;
                            break;
                        }
                        VerificationRequestState::Cancelled(info) => {
                            view.update(&mut cx, |this, cx| {
                                this.state = VerificationFlowState::Cancelled(info.clone());
                                cx.notify();
                            })?;
                            return anyhow::Ok(());
                        }
                        _ => {}
                    }
                }

                // later for qr support this will be interactive from the UI
                // like element allowing you to pick SAS/QR
                if request
                    .their_supported_methods()
                    .map(|m| m.contains(&VerificationMethod::SasV1))
                    .unwrap_or(false)
                {
                    Self::run_sas_flow(view, request, &mut cx).await?;
                }

                anyhow::Ok(())
            }
        })
        .detach();
    }

    // like start_verification, this was taken from CLI code and modified a bit.
    async fn run_sas_flow(
        view: WeakEntity<Self>,
        request: VerificationRequest,
        cx: &mut AsyncApp,
    ) -> anyhow::Result<()> {
        let sas = request
            .start_sas()
            .await?
            .context("Failed to transition into a SAS verification flow.")?;

        view.update(cx, |this, cx| {
            this.state = VerificationFlowState::Sas(SasFlowState::Started { sas: sas.clone() });
            cx.notify();
        })?;

        while let Some(state) = sas.changes().next().await {
            match state {
                SasState::KeysExchanged { emojis, .. } => {
                    if let Some(data) = emojis {
                        view.update(cx, |this, cx| {
                            this.state = VerificationFlowState::Sas(SasFlowState::KeysExchanged {
                                sas: sas.clone(),
                                emojis: data.emojis,
                            });
                            cx.notify();
                        })?;
                    }
                }
                SasState::Done { .. } => {
                    view.update(cx, |this, cx| {
                        this.state = VerificationFlowState::Completed;
                        // Mark task as completed in ExportApp
                        this.app
                            .update(cx, |app, _| {
                                app.todo_tasks.mark_completed("verification");
                            })
                            .ok();
                        cx.notify();
                    })?;
                    break;
                }
                SasState::Cancelled(info) => {
                    view.update(cx, |this, cx| {
                        this.state = VerificationFlowState::Cancelled(info);
                        cx.notify();
                    })?;
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// confirm that the auth string is matching
    fn confirm_sas(&mut self, cx: &mut Context<Self>) {
        if let VerificationFlowState::Sas(SasFlowState::KeysExchanged { sas, .. }) = &self.state {
            let sas = sas.clone();
            self.state = VerificationFlowState::Sas(SasFlowState::Confirming { sas: sas.clone() });
            cx.notify();

            cx.spawn(|_, _: &mut AsyncApp| async move {
                sas.confirm().await.ok();
            })
            .detach();
        }
    }

    /// cancel the request
    fn cancel(&mut self, cx: &mut Context<Self>) {
        match &self.state {
            VerificationFlowState::Requesting { request }
            | VerificationFlowState::Ready { request, .. } => {
                let request = request.clone();
                cx.spawn(|_, _: &mut AsyncApp| async move {
                    request.cancel().await.ok();
                })
                .detach();
            }
            VerificationFlowState::Sas(state) => {
                let sas = match state {
                    SasFlowState::Started { sas }
                    | SasFlowState::KeysExchanged { sas, .. }
                    | SasFlowState::Confirming { sas } => sas.clone(),
                };
                cx.spawn(|_, _: &mut AsyncApp| async move {
                    sas.cancel().await.ok();
                })
                .detach();
            }
            _ => {}
        }
    }
}

impl Render for VerificationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .overflow_hidden()
            .p_6()
            .child(match &self.state {
                VerificationFlowState::Idle => self.render_idle(cx),
                VerificationFlowState::Requesting { .. } => self.render_requesting(cx),
                VerificationFlowState::Ready { their_methods, .. } => {
                    self.render_ready(their_methods.clone(), cx)
                }
                VerificationFlowState::Sas(sas_state) => self.render_sas(sas_state, cx),
                VerificationFlowState::Completed => self.render_completed(cx),
                VerificationFlowState::Cancelled(info) => self.render_cancelled(info, cx),
            })
    }
}

// render methods for different stages of verification
// could've been done like other entities/views, currently
// this is like the initial version of the gui...
// idk if that's important enough rn, this works >_>
impl VerificationView {
    fn render_idle(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .max_w(rems(32.0))
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        gpui::div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("Verify this session"),
                    )
                    .child(
                        gpui::div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("This will enable cross-signing and request keys from the homeserver.")
                            .child("Key imports + cross-signing completes the encryption setup."), // It should anyway, assuming the server is healthy...
                    ),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_shrink()
                    .child(
                        Button::new("start_verification")
                            .primary()
                            .label("Start")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.client = view.app.read_with(cx, |app, _| app.user.client.clone()).ok().flatten();
                                view.start_verification(window, cx);
                            }))
                    ),
            )
            .into_any_element()
    }

    fn render_requesting(&mut self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .max_w(rems(32.0))
            .gap_4()
            .child(
                gpui::div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("Waiting for the other device..."),
            )
            .child(
                Button::new("cancel")
                    .outline()
                    .label("Cancel")
                    .on_click(cx.listener(|view, _, _, cx| view.cancel(cx))),
            )
            .into_any_element()
    }

    fn render_ready(
        &mut self,
        their_methods: Vec<VerificationMethod>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .max_w(rems(32.0))
            .gap_4()
            .child(
                gpui::div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child("Ready to verify"),
            )
            .child(
                gpui::div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Other device's methods: {:?}", their_methods)),
            )
            .into_any_element()
    }

    fn render_sas(&self, sas_state: &SasFlowState, cx: &mut Context<Self>) -> AnyElement {
        match sas_state {
            SasFlowState::Started { .. } => v_flex()
                .max_w(rems(32.0))
                .gap_4()
                .child(
                    gpui::div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child("Exchanging keys..."),
                )
                .into_any_element(),
            SasFlowState::KeysExchanged { emojis, .. } => v_flex()
                .max_w(rems(42.0))
                .gap_4()
                .child(
                    gpui::div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child("Do the emoji match?"),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .children(emojis.iter().map(|e| {
                            v_flex()
                                .items_center()
                                .p_2()
                                .child(gpui::div().text_2xl().child(e.symbol))
                                .child(
                                    gpui::div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(e.description),
                                )
                        })),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("confirm")
                                .primary()
                                .label("Yes")
                                .on_click(cx.listener(|view, _, _, cx| view.confirm_sas(cx))),
                        )
                        .child(
                            Button::new("cancel")
                                .outline()
                                .label("No")
                                .on_click(cx.listener(|view, _, _, cx| view.cancel(cx))),
                        ),
                )
                .into_any_element(),
            SasFlowState::Confirming { .. } => v_flex()
                .max_w(rems(32.0))
                .gap_4()
                .child(
                    gpui::div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child("Waiting for the other device to confirm..."),
                )
                .into_any_element(),
        }
    }

    fn render_completed(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .max_w(rems(32.0))
            .gap_4()
            .child(
                gpui::div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child(AppIcon::GeneralSuccess)
                    .child("Verification complete"),
            )
            .child(
                gpui::div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("This session is now verified."),
            )
            .into_any_element()
    }

    fn render_cancelled(&self, info: &CancelInfo, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .max_w(rems(32.0))
            .gap_4()
            .child(
                gpui::div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .child(AppIcon::GeneralError)
                    .child("Verification cancelled"),
            )
            .child(
                gpui::div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("Reason: {}", info.reason())),
            )
            .child(
                Button::new("retry")
                    .label("Try again")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.state = VerificationFlowState::Idle;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
