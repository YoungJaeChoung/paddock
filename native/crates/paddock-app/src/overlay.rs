//! 창 위에 떠 있는 묻기 창: 종류(`Overlay`), 열기·취소·확인·버리기, 그리기.
//!
//! 창마다 변종 하나와 `render_overlay`의 갈래 하나를 더한다. 키는 `window_view`의 키 처리가 `confirm_overlay`·`cancel_overlay`로 넘긴다.
use gpui_kit::{Context, Div, ParentElement, Window};
use paddock_core::entry_name::EntryNameCheck;
use paddock_core::file_tree::folder_label;
use paddock_core::git_status::Change;
use paddock_core::paths::file_name_of;
use paddock_core::theme::dialog as dialog_size;
use paddock_core::unsaved::{
    LOSE_CHANGES_DETAIL, overwrite_prompt, save_changes_title, unsaved_files_text, unsaved_suffix,
};
use paddock_core::work_model::{GroupKey, folder_name};

use crate::files::menu_model::entry_title;
use crate::quick_settings::model::QuickState;
use crate::quit_policy::default_quit_choice;
use crate::quit_prompt::{
    close_dialog, close_group_message, close_many_dialog, close_terminal_message, quit_dialog, remove_folder_detail,
    unsaved_dialog,
};
use crate::window_view::{RenameTarget, WorkWindow};
use crate::work::rename_dialog::{name_dialog, rename_dialog};

/// 창 위에 떠 있는 묻기 창.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// 터미널 하나를 닫을지 확인한다.
    CloseTerminal(u32),
    /// Unassigned 묶음의 터미널을 모두 닫을지 확인한다. 값은 묶음의 시작 터미널.
    CloseGroup(u32),
    /// 작업 폴더를 목록에서 빼며 그 폴더의 터미널을 모두 닫을지 확인한다.
    RemoveFolder(String),
    /// 터미널이나 폴더의 이름을 바꾼다.
    Rename(RenameTarget),
    /// `parent` 폴더 안에 새 파일(`is_dir`이 거짓)이나 새 폴더를 만들 이름을 받는다.
    NewEntry {
        parent: String,
        is_dir: bool,
    },
    /// 등록 계정을 보고 더하고 이름을 바꾸고 뺀다.
    Accounts,
    /// 작업 폴더의 변경 하나를 되돌릴지(추적 안 하는 새 파일이면 지울지) 확인한다.
    Discard(Change),
    /// 창을 닫을 때 열린 터미널을 어떻게 할지 묻는다.
    Quit,
    /// 저장하지 않은 파일 탭(보는 묶음 파일 칸의 `index`번째)을 닫을지 묻는다: `Save`·`Don't Save`·`Cancel`.
    CloseFile(usize),
    /// 저장하지 않은 파일 탭 중 다른 창(파일 분리 창)에 있는 것(열쇠)을 닫을지 묻는다: `Save`·`Don't Save`·`Cancel`. 묻기 창은 본 창에 열린다.
    CloseDetachedFile(String),
    /// 창을 닫을 때 저장하지 않은 파일이 있음을 알린다: `Save all`·`Discard`·`Cancel`.
    QuitUnsaved,
    /// 디스크의 파일이 열었을 때와 달라져 있는데 이 파일(경로)을 저장해도 되는지 묻는다.
    OverwriteFile(String),
    /// 고른 줄에 검토 메모를 남긴다(Ctrl+Alt+M·편집기 메뉴). 값은 메모가 가리킬 파일(절대 경로)·줄 범위·시작 줄 내용이다.
    ReviewNote {
        file: String,
        start_line: usize,
        end_line: usize,
        line_text: String,
    },
    /// 파일 빠른 열기(Ctrl+Shift+P): 이름으로 파일을 골라 연다.
    QuickOpen,
    /// 폴더 검색 결과 전체에 바꾸기를 할지 확인한다(`Replace All`). 값은 확인 문장에 보일 일치 줄 수와 파일 수다.
    ReplaceAll {
        n_matches: usize,
        n_files: usize,
    },
    /// 줄 번호를 받아 그 줄로 간다(Ctrl+G). `n_lines`는 창을 열 때의 편집기 줄 수(고치는 중이라 읽어 둔 값과 다를 수 있다).
    GoToLine {
        n_lines: usize,
    },
    /// WSL 터미널에도 Claude 사용량을 보이도록 `~/.bashrc`에 줄을 넣어도 되는지 묻는다(Windows + WSL, 설정이 `ask`일 때 시작 때 한 번).
    WslLauncher,
}

impl Overlay {
    /// 자동 점검 상태 글에 적는 이름.
    #[cfg(feature = "automation")]
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Overlay::None => "none",
            Overlay::CloseTerminal(_) => "close-terminal",
            Overlay::CloseGroup(_) => "close-group",
            Overlay::RemoveFolder(_) => "remove-folder",
            Overlay::Rename(_) => "rename",
            Overlay::NewEntry { .. } => "new-entry",
            Overlay::Accounts => "accounts",
            Overlay::Discard(_) => "discard",
            Overlay::Quit => "quit",
            Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_) => "close-file",
            Overlay::QuitUnsaved => "quit-unsaved",
            Overlay::OverwriteFile(_) => "overwrite-file",
            Overlay::GoToLine { .. } => "goto-line",
            Overlay::ReviewNote { .. } => "review-note",
            Overlay::QuickOpen => "quick-open",
            Overlay::ReplaceAll { .. } => "replace-all",
            Overlay::WslLauncher => "wsl-launcher",
        }
    }
}

impl WorkWindow {
    pub(crate) fn open_overlay(
        &mut self,
        overlay: Overlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = overlay;
        self.menu = None;
        self.quick = QuickState::default();
        self.remember_choice = false;
        window.focus(&self.overlay_focus, cx);
        cx.notify();
    }

    pub(crate) fn cancel_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_overwrite = matches!(self.overlay, Overlay::OverwriteFile(_));
        self.overlay = Overlay::None;
        self.rename_input = None;
        self.entry_input = None;
        self.goto_input = None;
        self.note_input = None;
        self.quick_open = None;
        self.quit_unsaved_resolved = false;
        // 덮어쓰기 묻기를 취소하면 그 저장에 딸린 탭 닫기·종료는 멈추고 기다리던 묻기도 접는다(띠는 남아 있다).
        if was_overwrite {
            self.close_after_save.clear();
            self.quit_saving.clear();
            self.overwrite_queue.clear();
        }
        self.clear_accounts();
        self.show_next_overwrite(window, cx);
        if self.overlay == Overlay::None {
            // 분리 창의 파일 단축키가 연 묻기 창이었으면 키 입력은 그 분리 창으로 돌아간다.
            if !self.end_detached_file_context(cx) {
                self.focus_selected(window, cx);
            }
        }
        cx.notify();
    }

    /// 묻기 창의 기본 선택(Enter)을 따른다: 터미널 닫기는 닫기, 종료 묻기는 백그라운드 유지.
    pub(crate) fn confirm_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.overlay.clone() {
            Overlay::CloseTerminal(id) => {
                self.overlay = Overlay::None;
                self.close_terminal(id, window, cx);
            }
            Overlay::CloseGroup(root) => {
                self.overlay = Overlay::None;
                self.close_group(root, window, cx);
            }
            Overlay::RemoveFolder(key) => {
                self.overlay = Overlay::None;
                self.remove_folder(&key, window, cx);
            }
            Overlay::Rename(_) => self.commit_rename(window, cx),
            Overlay::NewEntry { .. } => self.commit_new_entry(window, cx),
            Overlay::Discard(change) => self.scm_confirm_discard(&change, window, cx),
            // Accounts 창의 Enter는 입력칸이 알리는 확정 사건과 단추가 다룬다.
            Overlay::Accounts => {}
            Overlay::Quit => {
                // 앱이 닫혀도 터미널이 남지 못하는 환경에서는 기본 선택이 없다(Enter로 끝낼 수 없다).
                if let Some(choice) = default_quit_choice(self.breakaway_denied) {
                    self.choose_quit(choice, window, cx);
                }
            }
            Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_) => self.close_file_saving(window, cx),
            Overlay::QuitUnsaved => self.quit_saving_all(window, cx),
            Overlay::OverwriteFile(path) => self.save_overwriting(&path, window, cx),
            Overlay::GoToLine { .. } => self.commit_go_to_line(window, cx),
            Overlay::ReviewNote { .. } => self.commit_review_note(window, cx),
            Overlay::QuickOpen => self.quick_open_choose(window, cx),
            Overlay::ReplaceAll { .. } => self.replace_all(window, cx),
            // 사용자 파일을 고치는 선택이 Enter 한 번으로 되지 않게, 기본 선택은 "나중에"(그대로 두고 다음 시작에 다시 묻기)다.
            Overlay::WslLauncher => self.cancel_overlay(window, cx),
            Overlay::None => {}
        }
    }

    /// 저장 여부 묻기 창의 버리기 단추(`Don't Save`·`Discard`).
    pub(crate) fn discard_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.overlay {
            Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_) => self.close_file_discarding(window, cx),
            Overlay::QuitUnsaved => self.quit_discarding(window, cx),
            _ => {}
        }
    }

    /// 열린 묻기 창을 `root` 위에 얹어 돌려준다.
    pub(crate) fn render_overlay(
        &self,
        mut root: Div,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        match self.overlay.clone() {
            Overlay::None => {}
            Overlay::CloseTerminal(id) => {
                let name = self.workspace.name(id).to_string();
                let message = close_terminal_message(&name, self.files_closed_with(id));
                let message = format!("{message}{}", unsaved_suffix(self.n_unsaved_closed_with(id)));
                root = root.child(close_dialog(&message, &self.overlay_focus, cx));
            }
            Overlay::CloseGroup(group_root) => {
                let n_terminals = self.workspace.work().group_members(group_root).len();
                let n_files = self.workspace.files().n_open_in(&GroupKey::Group(group_root));
                root = root.child(close_many_dialog(
                    "Close terminals",
                    &format!(
                        "{}{}",
                        close_group_message(n_terminals, n_files),
                        unsaved_suffix(self.n_unsaved_in(&GroupKey::Group(group_root)))
                    ),
                    "The shells and anything running in them will end.",
                    "Close terminals",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::RemoveFolder(key) => {
                let n_terminals = self.workspace.work().terminals_of(&key).len();
                let n_files = self.workspace.files().n_open_in(&GroupKey::Folder(key.clone()));
                let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
                let name = folder_name(&key, self.workspace.work().folder_label(&key), home.as_deref());
                root = root.child(close_many_dialog(
                    "Remove from list",
                    &format!(
                        "Remove \"{name}\" from the list?{}",
                        unsaved_suffix(self.n_unsaved_in(&GroupKey::Folder(key.clone())))
                    ),
                    &remove_folder_detail(n_terminals, n_files),
                    "Remove from list",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::Rename(target) => {
                if let Some(input) = self.rename_input.clone() {
                    root = root.child(rename_dialog(&target, &input, &self.overlay_focus, cx));
                }
            }
            Overlay::NewEntry { parent, is_dir } => {
                if let Some(input) = self.entry_input.clone() {
                    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok();
                    let label = folder_label(&self.workspace.work().folders, &parent, home.as_deref());
                    let check = self.entry_name_check(cx);
                    let reason = match check {
                        EntryNameCheck::Invalid(reason) => Some(reason),
                        EntryNameCheck::Valid | EntryNameCheck::Empty => None,
                    };
                    root = root.child(name_dialog(
                        entry_title(is_dir, &label),
                        &input,
                        reason,
                        check == EntryNameCheck::Valid,
                        &self.overlay_focus,
                        WorkWindow::commit_new_entry,
                        cx,
                    ));
                }
            }
            Overlay::Discard(change) => {
                let prompt = crate::scm::model::discard_prompt(&change);
                root = root.child(close_many_dialog(
                    prompt.title,
                    &prompt.headline,
                    prompt.detail,
                    prompt.confirm,
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::Accounts => {
                if let Some(model) = self.accounts.model.clone() {
                    let max_height = (f32::from(window.viewport_size().height) / crate::look::zoom()
                        - paddock_core::theme::accounts_dialog::SCREEN_MARGIN
                        - dialog_size::TITLE_HEIGHT
                        - 2.0 * dialog_size::CONTENT_PADDING
                        - 70.0)
                        .max(120.0);
                    let inputs = crate::accounts::dialog::AccountInputs {
                        name: self.accounts.name_input.as_ref(),
                        rename: self.accounts.rename_input.as_ref(),
                    };
                    root = root.child(crate::accounts::dialog::accounts_dialog(
                        &model,
                        inputs,
                        &self.overlay_focus,
                        max_height,
                        cx,
                    ));
                }
            }
            Overlay::Quit => root = root.child(quit_dialog(self, &self.overlay_focus, cx)),
            Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_) => {
                if let Some(path) = self.close_prompt_path() {
                    root = root.child(unsaved_dialog(
                        "Unsaved changes",
                        &save_changes_title(&file_name_of(&path)),
                        Some(LOSE_CHANGES_DETAIL),
                        "Save",
                        "Don't Save",
                        &self.overlay_focus,
                        cx,
                    ));
                }
            }
            Overlay::QuitUnsaved => {
                let mut paths = self.unsaved.paths();
                let headline = match (paths.next(), paths.next()) {
                    (Some(only), None) => save_changes_title(&file_name_of(only)),
                    _ => unsaved_files_text(self.unsaved.len()),
                };
                root = root.child(unsaved_dialog(
                    "Quit Paddock",
                    &headline,
                    Some(LOSE_CHANGES_DETAIL),
                    "Save all",
                    "Discard",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::OverwriteFile(path) => {
                root = root.child(close_many_dialog(
                    "Overwrite file",
                    &overwrite_prompt(&file_name_of(&path)),
                    "Your version replaces what is on disk.",
                    "Overwrite",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::ReplaceAll { n_matches, n_files } => {
                root = root.child(close_many_dialog(
                    "Replace in Files",
                    &paddock_core::search_model::replace_all_prompt(n_matches, n_files),
                    "The files are changed on disk. Files with unsaved changes are skipped.",
                    "Replace All",
                    &self.overlay_focus,
                    cx,
                ));
            }
            Overlay::WslLauncher => {
                root = root.child(crate::usage::launcher_dialog::wsl_launcher_dialog(&self.overlay_focus, cx));
            }
            Overlay::QuickOpen => {
                if let Some(dialog) = crate::quick_open::view::quick_open_dialog(self, cx) {
                    root = root.child(dialog);
                }
            }
            Overlay::ReviewNote { start_line, end_line, .. } => {
                if let Some(input) = self.note_input.clone() {
                    root = root.child(name_dialog(
                        paddock_core::review_notes::prompt_title(start_line, end_line),
                        &input,
                        None,
                        self.review_note_text(cx).is_some(),
                        &self.overlay_focus,
                        WorkWindow::commit_review_note,
                        cx,
                    ));
                }
            }
            Overlay::GoToLine { n_lines } => {
                if let Some(input) = self.goto_input.clone() {
                    root = root.child(name_dialog(
                        format!("{} (1\u{2013}{n_lines})", paddock_core::theme::editor::GO_TO_LINE_TITLE),
                        &input,
                        None,
                        self.go_to_line_target(cx).is_some(),
                        &self.overlay_focus,
                        WorkWindow::commit_go_to_line,
                        cx,
                    ));
                }
            }
        }
        root
    }
}
