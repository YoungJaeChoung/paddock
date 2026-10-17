//! 창을 닫을 때 열린 터미널을 어떻게 할지 묻는 창과, 터미널 하나를 닫기 전 확인하는 창.
//!
//! 별도 창이 아니라 앱 창 위에 띄운다 — 별도 창은 다른 창 뒤나 화면 구석에 떠서 놓치기 쉽고, 놓치면 앱이 창 없이 남는다.
//! 문구는 영어이며, 종료 묻기 문구와 선택지는 Electron판 Paddock의 종료 묻기와 같다.
use gpui_kit::{
    Context, FocusHandle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
};

use paddock_core::theme::dialog as dialog_size;

use crate::chrome::{ButtonKind, button, checkbox, dialog, zpx};
use crate::look::colors;
use crate::quit_policy::{OnQuit, default_quit_choice};
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 열린 터미널 수를 알리는 문장.
///
/// Examples
/// --------
/// | n_terminals | 결과                         |
/// |-------------|------------------------------|
/// | 1           | `1 terminal is still running.` |
/// | 3           | `3 terminals are still running.` |
pub fn running_message(n_terminals: usize) -> String {
    if n_terminals == 1 {
        "1 terminal is still running.".to_string()
    } else {
        format!("{n_terminals} terminals are still running.")
    }
}

/// 종료 묻기의 설명. 앱이 닫혀도 터미널이 남지 못하는 환경이면 그 사실을 알린다.
/// 문구는 기존 앱의 종료 묻기와 같다.
pub fn quit_detail(breakaway_denied: bool) -> &'static str {
    if breakaway_denied {
        "Terminals will stop when Paddock closes."
    } else {
        "Keep them running to pick up where you left off the next time you open Paddock. You can change this later in Settings under \"Backend: On Quit\" (paddock.backend.onQuit)."
    }
}

/// 종료 묻기 창. "Don't ask again"을 켜고 고르면 그 선택이 설정에 저장된다.
/// 앱이 닫혀도 터미널이 남지 못하는 환경에서는 남기는 선택(`Keep running in background`)을 보이지 않는다.
pub fn quit_dialog(
    window: &WorkWindow,
    focus: &FocusHandle,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let remember = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(dialog_size::CONTENT_GAP))
        .cursor_pointer()
        .child(checkbox(window.remember_choice))
        .child("Don't ask again")
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                this.remember_choice = !this.remember_choice;
                cx.notify();
            }),
        );
    let cancel = button("Cancel", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
    );
    let stop = button("Stop all and quit", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.choose_quit(OnQuit::Stop, window, cx)),
    );
    let mut buttons = div()
        .flex()
        .flex_row()
        .justify_end()
        .pt(zpx(dialog_size::CONTENT_PADDING))
        .gap(zpx(dialog_size::BUTTON_GAP))
        .child(cancel)
        .child(stop);
    if default_quit_choice(window.breakaway_denied).is_some() {
        buttons = buttons.child(button("Keep running in background", ButtonKind::Primary).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| this.choose_quit(OnQuit::Keep, window, cx)),
        ));
    }
    let body = div()
        .flex()
        .flex_col()
        .gap(zpx(dialog_size::CONTENT_GAP))
        .child(running_message(window.n_terminals()))
        .child(div().text_color(hsla(palette.fg)).child(quit_detail(window.breakaway_denied)))
        .child(remember)
        .child(buttons);
    dialog(focus, "Quit Paddock", body)
}

/// 터미널 하나를 닫기 전 확인 창. `headline`이 닫을 터미널의 이름(과 함께 닫히는 파일 탭 수)을 말한다.
pub fn close_dialog(
    headline: &str,
    focus: &FocusHandle,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let cancel = button("Cancel", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
    );
    let close = button("Close terminal", ButtonKind::Destructive).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.confirm_overlay(window, cx)),
    );
    let body = div()
        .flex()
        .flex_col()
        .gap(zpx(dialog_size::CONTENT_GAP))
        .child(headline.to_string())
        .child(div().text_color(hsla(palette.fg)).child("The shell and anything running in it will end."))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .pt(zpx(dialog_size::CONTENT_PADDING))
                .gap(zpx(dialog_size::BUTTON_GAP))
                .child(cancel)
                .child(close),
        );
    dialog(focus, "Close terminal", body)
}

/// 터미널 여럿을 한꺼번에 닫는 확인 창(Unassigned 묶음 닫기·목록에서 폴더 빼기).
/// `headline`이 무엇을 닫는지 말하고 `detail`이 닫으면 무슨 일이 일어나는지 말한다.
pub fn close_many_dialog(
    title: &'static str,
    headline: &str,
    detail: &str,
    confirm_label: &'static str,
    focus: &FocusHandle,
    cx: &mut Context<WorkWindow>,
) -> impl IntoElement {
    let palette = colors();
    let cancel = button("Cancel", ButtonKind::Secondary).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.cancel_overlay(window, cx)),
    );
    let confirm = button(confirm_label, ButtonKind::Destructive).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _event: &MouseDownEvent, window, cx| this.confirm_overlay(window, cx)),
    );
    let body = div()
        .flex()
        .flex_col()
        .gap(zpx(dialog_size::CONTENT_GAP))
        .child(headline.to_string())
        .child(div().text_color(hsla(palette.fg)).child(detail.to_string()))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .pt(zpx(dialog_size::CONTENT_PADDING))
                .gap(zpx(dialog_size::BUTTON_GAP))
                .child(cancel)
                .child(confirm),
        );
    dialog(focus, title, body)
}

/// 파일 탭 수를 덧붙이는 말(`and 2 files`). 파일 탭이 없으면 빈 글자다.
fn files_phrase(n_files: usize) -> String {
    match n_files {
        0 => String::new(),
        1 => " and 1 file".to_string(),
        n => format!(" and {n} files"),
    }
}

/// 터미널 하나를 닫는 확인의 머리 문장. Unassigned 묶음의 마지막 터미널이면 묶음의 파일 탭도 함께 닫히므로 그 수를 말한다.
///
/// Examples
/// --------
/// | name         | n_files | 결과                               |
/// |--------------|---------|------------------------------------|
/// | `terminal 1` | 0       | `Close "terminal 1"?`              |
/// | `terminal 1` | 1       | `Close "terminal 1" and 1 file?`   |
/// | `terminal 1` | 3       | `Close "terminal 1" and 3 files?`  |
pub fn close_terminal_message(
    name: &str,
    n_files: usize,
) -> String {
    format!("Close \"{name}\"{}?", files_phrase(n_files))
}

/// 작업 폴더를 목록에서 뺄 때 확인 창이 말하는, 함께 끝나는 것들. 터미널과 파일 탭 수를 말하고 디스크의 폴더는 그대로임을 덧붙인다.
///
/// Examples
/// --------
/// | n_terminals | n_files | 결과                                                         |
/// |-------------|---------|--------------------------------------------------------------|
/// | 1           | 0       | `Its 1 terminal will end. The folder on disk is kept.`       |
/// | 2           | 1       | `Its 2 terminals and 1 file will end. The folder on disk is kept.` |
/// | 0           | 3       | `Its 3 files will close. The folder on disk is kept.`        |
pub fn remove_folder_detail(
    n_terminals: usize,
    n_files: usize,
) -> String {
    let terminals = format!("{n_terminals} terminal{}", if n_terminals == 1 { "" } else { "s" });
    let files = format!("{n_files} file{}", if n_files == 1 { "" } else { "s" });
    let (subject, verb) = match (n_terminals, n_files) {
        (_, 0) => (terminals, "end"),
        (0, _) => (files, "close"),
        _ => (format!("{terminals} and {files}"), "end"),
    };
    format!("Its {subject} will {verb}. The folder on disk is kept.")
}

/// 터미널 여럿을 닫는 확인의 머리 문장. 묶음의 파일 탭도 함께 닫히므로 있으면 그 수를 말한다.
///
/// Examples
/// --------
/// | n_terminals | n_files | 결과                                         |
/// |-------------|---------|----------------------------------------------|
/// | 1           | 0       | `Close 1 terminal in this group?`            |
/// | 3           | 0       | `Close 3 terminals in this group?`           |
/// | 3           | 2       | `Close 3 terminals and 2 files in this group?` |
pub fn close_group_message(
    n_terminals: usize,
    n_files: usize,
) -> String {
    format!(
        "Close {n_terminals} terminal{}{} in this group?",
        if n_terminals == 1 { "" } else { "s" },
        files_phrase(n_files)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_message_counts_terminals() {
        assert_eq!(running_message(1), "1 terminal is still running.");
        assert_eq!(running_message(3), "3 terminals are still running.");
    }

    #[test]
    fn test_close_group_message_counts_terminals_and_files() {
        assert_eq!(close_group_message(1, 0), "Close 1 terminal in this group?");
        assert_eq!(close_group_message(3, 0), "Close 3 terminals in this group?");
        assert_eq!(close_group_message(3, 2), "Close 3 terminals and 2 files in this group?");
        assert_eq!(close_group_message(1, 1), "Close 1 terminal and 1 file in this group?");
    }

    #[test]
    fn test_c_d51_removing_a_folder_names_the_files_that_close_with_its_terminals() {
        assert_eq!(remove_folder_detail(1, 0), "Its 1 terminal will end. The folder on disk is kept.");
        assert_eq!(remove_folder_detail(2, 1), "Its 2 terminals and 1 file will end. The folder on disk is kept.");
        assert_eq!(remove_folder_detail(0, 3), "Its 3 files will close. The folder on disk is kept.");
    }

    #[test]
    fn test_c_d51_closing_the_last_terminal_of_a_group_names_the_files_that_close_with_it() {
        assert_eq!(close_terminal_message("terminal 1", 0), "Close \"terminal 1\"?");
        assert_eq!(close_terminal_message("terminal 1", 1), "Close \"terminal 1\" and 1 file?");
        assert_eq!(close_terminal_message("terminal 1", 3), "Close \"terminal 1\" and 3 files?");
    }

    #[test]
    fn test_quit_detail_warns_when_terminals_cannot_outlive_the_app() {
        assert_eq!(quit_detail(true), "Terminals will stop when Paddock closes.");
        assert!(quit_detail(false).starts_with("Keep them running"));
        // 기존 앱의 종료 묻기와 같은 안내 문구다.
        assert!(quit_detail(false).ends_with("under \"Backend: On Quit\" (paddock.backend.onQuit)."));
    }
}
