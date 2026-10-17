//! Accounts 창을 그린다: 등록 계정 목록(열기·이름 바꾸기·제거), 계정 추가 양식, 오류 상자.
//!
//! 상태와 문구 규칙은 `model`이, 파일 읽기·쓰기는 `store`가 쥔다. 이 파일은 상태를 그림으로 바꾸고 누름을 창(`WorkWindow`)의
//! `accounts_*` 동작에 이어 줄 뿐이다. 이름 입력칸은 gpui-component의 입력칸이라 한글 조합 중 Enter는 입력칸이 알리는 확정 사건으로 받는다.
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{
    Context, Div, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div,
};
use paddock_core::theme::accounts_dialog as size;
use paddock_core::theme::text;

use super::model::{AccountsModel, Editing, PROVIDERS, runtime_text, title_of};
use crate::chrome::{ButtonKind, button, dialog_sized, zpx};
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 창에 쓰이는 입력칸 둘(추가 양식의 이름, 이름 바꾸기).
pub struct AccountInputs<'a> {
    pub name: Option<&'a Entity<InputState>>,
    pub rename: Option<&'a Entity<InputState>>,
}

/// 입력칸을 테두리 상자로 감싼다.
fn field_box(input: &Entity<InputState>) -> Div {
    let palette = colors();
    div()
        .h(zpx(size::FIELD_HEIGHT))
        .w_full()
        .rounded(zpx(size::FIELD_RADIUS))
        .border_1()
        .border_color(hsla(palette.accent))
        .bg(hsla(palette.control_bg))
        .child(Input::new(input).appearance(false))
}

/// 이름표가 붙은 칸 하나.
fn labelled(
    label: &'static str,
    content: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(zpx(size::FIELD_MIN_WIDTH))
        .gap(zpx(size::FIELD_LABEL_GAP))
        .child(div().text_size(zpx(size::FIELD_LABEL_SIZE)).text_color(hsla(colors().fg_muted)).child(label))
        .child(content)
}

/// 목록 안 버튼(보조 크기 30).
fn small_button(
    label: &'static str,
    kind: ButtonKind,
) -> Div {
    button(label, kind).h(zpx(size::BUTTON_HEIGHT)).px(zpx(size::BUTTON_PADDING_X)).items_center()
}

/// 여러 선택지 중 하나를 고르는 단추 묶음의 단추 하나.
fn choice_button(
    label: String,
    selected: bool,
) -> Div {
    let palette = colors();
    div()
        .h(zpx(size::BUTTON_HEIGHT))
        .px(zpx(size::BUTTON_PADDING_X))
        .flex()
        .items_center()
        .rounded(zpx(size::BUTTON_RADIUS))
        .border_1()
        .border_color(hsla(if selected { palette.accent } else { palette.border }))
        .bg(hsla(if selected { palette.selected_bg } else { palette.control_bg }))
        .text_color(hsla(if selected { palette.fg_strong } else { palette.fg }))
        .cursor_pointer()
        .child(label)
}

/// 목록의 계정 한 줄.
fn profile_row(
    model: &AccountsModel,
    profile: &paddock_core::agent_account::AccountProfile,
    rename_input: Option<&Entity<InputState>>,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let id = profile.id.clone();
    let summary = div()
        .flex_1()
        .min_w(zpx(size::SUMMARY_MIN_WIDTH))
        .flex()
        .flex_col()
        .gap(zpx(size::SUMMARY_GAP))
        .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(title_of(profile)))
        .child(div().text_size(zpx(size::DETAIL_SIZE)).text_color(hsla(palette.fg_muted)).child(runtime_text(profile)));
    let row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(zpx(size::ROW_GAP))
        .p(zpx(size::ROW_PADDING))
        .rounded(zpx(size::ROW_RADIUS))
        .border_1()
        .border_color(hsla(palette.border))
        .bg(hsla(palette.control_bg))
        .child(summary);
    match &model.editing {
        Editing::Rename(editing) if *editing == profile.id => {
            let (save_id, cancel_id) = (id.clone(), id);
            let mut form = div().flex().flex_row().items_end().gap(zpx(size::BUTTON_GAP)).w_full();
            if let Some(input) = rename_input {
                form = form.child(labelled("Account name", field_box(input)));
            }
            row.child(
                form.child(small_button("Save", ButtonKind::Primary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        this.accounts_save_rename(&save_id, window, cx)
                    }),
                ))
                .child(small_button("Cancel", ButtonKind::Secondary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        let _ = &cancel_id;
                        this.accounts_cancel_edit(window, cx)
                    }),
                )),
            )
        }
        Editing::Remove(removing) if *removing == profile.id => {
            let remove_id = id;
            row.child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(zpx(size::BUTTON_GAP))
                    .child("Remove this account from the list? Existing terminals, login data, and conversations will stay on this device.")
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(zpx(size::BUTTON_GAP))
                            .child(small_button("Remove", ButtonKind::Destructive).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                                    this.accounts_confirm_remove(&remove_id, window, cx)
                                }),
                            ))
                            .child(small_button("Cancel", ButtonKind::Secondary).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                                    this.accounts_cancel_edit(window, cx)
                                }),
                            )),
                    ),
            )
        }
        _ => {
            let (open_id, rename_id, remove_id) = (id.clone(), id.clone(), id);
            row.child(
                div()
                    .flex()
                    .flex_row()
                    .gap(zpx(size::BUTTON_GAP))
                    .child(small_button("Open", ButtonKind::Primary).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                            this.accounts_open(&open_id, window, cx)
                        }),
                    ))
                    .child(small_button("Rename", ButtonKind::Secondary).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                            this.accounts_begin_rename(&rename_id, window, cx)
                        }),
                    ))
                    .child(small_button("Remove", ButtonKind::Secondary).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                            this.accounts_begin_remove(&remove_id, window, cx)
                        }),
                    )),
            )
        }
    }
}

/// 계정 추가 양식: 도구·이름·(선택지가 둘 이상이면) 실행 환경, 안내 한 줄, 추가 단추.
fn add_form(
    model: &AccountsModel,
    name_input: Option<&Entity<InputState>>,
    cx: &mut Context<WorkWindow>,
) -> Div {
    let palette = colors();
    let mut providers = div().flex().flex_row().gap(zpx(size::BUTTON_GAP));
    for provider in PROVIDERS {
        let label = paddock_core::account_profile::provider_name(provider).unwrap_or("").to_string();
        providers = providers.child(choice_button(label, model.provider == provider).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                this.accounts_set_provider(provider, window, cx)
            }),
        ));
    }
    let mut fields = div().flex().flex_row().flex_wrap().gap(zpx(size::FIELD_GAP)).child(labelled("Tool", providers));
    if let Some(input) = name_input {
        fields = fields.child(labelled("Account name", field_box(input)));
    }
    if model.choices.len() > 1 {
        let mut runtimes = div().flex().flex_row().flex_wrap().gap(zpx(size::BUTTON_GAP));
        for (index, choice) in model.choices.iter().enumerate() {
            runtimes = runtimes.child(choice_button(choice.label.clone(), index == model.choice_index).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                    this.accounts_set_runtime(index, window, cx)
                }),
            ));
        }
        fields = fields.child(labelled("Run in", runtimes));
    }
    let mut actions = div().flex().flex_row().gap(zpx(size::BUTTON_GAP)).child(
        small_button("Add & Open", ButtonKind::Primary).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.accounts_add(window, cx)),
        ),
    );
    if !model.profiles.is_empty() {
        actions = actions.child(small_button("Cancel", ButtonKind::Secondary).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.accounts_cancel_form(window, cx)),
        ));
    }
    div()
        .flex()
        .flex_col()
        .gap(zpx(size::FIELD_GAP))
        .pt(zpx(size::SECTION_GAP))
        .border_t_1()
        .border_color(hsla(palette.border))
        .child(div().text_size(zpx(size::FORM_HEADING_SIZE)).font_weight(FontWeight::SEMIBOLD).child("Add account"))
        .child(fields)
        .child(
            div()
                .text_size(zpx(size::HINT_SIZE))
                .text_color(hsla(palette.fg_muted))
                .child("Open the account once in the Electron Paddock to link skills and settings. Sign in through Claude or Codex in the new terminal."),
        )
        .child(actions)
}

/// Accounts 창.
pub fn accounts_dialog(
    model: &AccountsModel,
    inputs: AccountInputs,
    focus: &FocusHandle,
    max_height: f32,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let mut content = div()
        .id("accounts-content")
        .flex()
        .flex_col()
        .gap(zpx(size::SECTION_GAP))
        .px(zpx(size::PADDING_X))
        .pt(zpx(size::PADDING_TOP))
        .max_h(zpx(max_height))
        .overflow_y_scroll()
        .child(
            div()
                .text_color(hsla(palette.fg_muted))
                .child("Start a new conversation with any account. Your existing terminals stay open."),
        );
    if !model.profiles.is_empty() {
        let mut list = div().flex().flex_col().gap(zpx(size::LIST_GAP));
        for profile in &model.profiles {
            list = list.child(profile_row(model, profile, inputs.rename, cx));
        }
        content = content.child(list);
    } else if !model.load_failed && !model.form_visible {
        content = content.child(div().text_color(hsla(palette.fg_muted)).child("No accounts added yet."));
    }
    if model.form_visible && !model.load_failed {
        content = content.child(add_form(model, inputs.name, cx));
    } else if !model.load_failed {
        content = content.child(div().flex().flex_row().child(
            small_button("Add account", ButtonKind::Secondary).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| this.accounts_show_form(window, cx)),
            ),
        ));
    }
    if let Some(error) = &model.error {
        let mut box_ = div()
            .flex()
            .flex_col()
            .gap(zpx(size::BUTTON_GAP))
            .px(zpx(size::ERROR_PADDING_X))
            .py(zpx(size::ERROR_PADDING_Y))
            .rounded(zpx(size::ROW_RADIUS))
            .border_1()
            .border_color(hsla(palette.danger))
            .bg(hsla(palette.control_bg))
            .text_size(zpx(text::UI))
            .text_color(hsla(palette.danger))
            .child(error.clone());
        if model.load_failed {
            box_ =
                box_.child(div().flex().flex_row().child(small_button("Retry", ButtonKind::Secondary).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, window, cx| this.accounts_retry(window, cx)),
                )));
        }
        content = content.child(box_);
    }
    let footer = div().flex().flex_row().justify_end().px(zpx(size::PADDING_X)).pt(zpx(size::SECTION_GAP)).child(
        small_button("Close", ButtonKind::Secondary).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
        ),
    );
    dialog_sized(focus, "Accounts", size::WIDTH, div().flex().flex_col().child(content).child(footer))
}
