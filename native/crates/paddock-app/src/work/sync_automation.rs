//! 자동 점검용: 동기화 동작(더블클릭 이름 바꾸기·행 끌기·`Show to the Side`·탭 줄 초점·메뉴 강조·터미널 찾기·사이드바 들여쓰기·선택 자동 복사)의
//! 대본 명령(`act`)과 창 상태 글 줄. `automation` 기능으로 빌드한 개발용 실행 파일에만 들어간다.
use gpui_kit::{Context, Window};
use paddock_core::incoming::{ClipboardPart, Incoming, incoming_of};
use paddock_core::work_model::DropPosition;

use super::terminal_row::RowTarget;
use crate::window_view::{RenameTarget, WorkWindow};

/// `경로|경로…`로 이어 쓴 `act` 인자를 경로 목록으로(경로에 빈칸이 있어도 되게 `|`로 가른다).
fn paths_of(parts: &[&str]) -> Vec<String> {
    parts.join(" ").split('|').map(str::trim).filter(|path| !path.is_empty()).map(str::to_string).collect()
}

impl WorkWindow {
    /// 창 상태 글에 붙는 동기화 줄: `tab-bar-focus=`·`menu-highlight=`·`menu-item N 글자`·`term-find …`·`sidebar-indent=`·`copy-on-selection=`·`file-tab-label N 글자`.
    pub(crate) fn sync_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        let mut lines = vec![
            format!("files-selected={}", self.files.selected_row().unwrap_or("-")),
            format!("files-focus={}", if self.files.list_focused() { "yes" } else { "no" }),
            format!(
                "rename-target={}",
                match &self.overlay {
                    crate::window_view::Overlay::Rename(crate::window_view::RenameTarget::Entry { path, .. }) => {
                        format!("entry:{path}")
                    }
                    _ => "-".to_string(),
                }
            ),
            self.rename_summary(cx),
            format!("clipboard={}", crate::automation::last_clipboard()),
            format!("status={}", self.status.as_deref().unwrap_or("-")),
            format!("tab-bar-focus={}", if self.tab_bar_focused { "yes" } else { "no" }),
            format!(
                "menu-highlight={}",
                self.menu.as_ref().and_then(|menu| menu.nav.highlight).map_or("-".to_string(), |at| at.to_string())
            ),
            format!("sidebar-indent={}", crate::look::sidebar_indent()),
            format!("copy-on-selection={}", if crate::look::copy_on_selection() { "on" } else { "off" }),
        ];
        // 탭 줄이 열마다 그리는 탭(`:faded`는 다른 창에 있는 흐린 탭). 칸을 나눈 줄에서는 다른 창의 탭이 빠진다.
        let (width, height) = match self.body_size.get() {
            (0.0, _) | (_, 0.0) => (1000.0, 600.0),
            measured => measured,
        };
        for (index, column) in self.workspace.strip_columns(width, height).iter().enumerate() {
            let tabs: Vec<String> = column
                .iter()
                .map(|(id, detached)| if *detached { format!("{id}:faded") } else { id.to_string() })
                .collect();
            lines.push(format!(
                "strip col{index} tabs={}",
                if tabs.is_empty() { "-".to_string() } else { tabs.join(",") }
            ));
        }
        // 탭 끌기: 놓을 자리 미리보기(`<방식>:<기준 탭>`)와 끌고 있는 터미널 번호.
        lines.push(format!("dock-preview={}", self.dock_preview_summary()));
        lines.push(format!("drag={}", self.drag_summary()));
        if let Some(menu) = self.menu.as_ref() {
            for (index, item) in self.menu_entries(&menu.kind).iter().enumerate() {
                lines.push(format!("menu-item {} {}", index + 1, item.label));
            }
        }
        let find = self.find.as_ref().map(|bar| {
            let grid = self.workspace.tab(bar.terminal).map(|tab| tab.content.session.grid.clone());
            let (offset, selection) = grid.map_or((0, "-".to_string()), |grid| {
                let term = grid.term.lock();
                (term.grid().display_offset(), term.selection_to_string().unwrap_or_else(|| "-".to_string()))
            });
            format!(
                "term-find open=y terminal={} query={} count={} matches={} offset={offset} selection={}",
                bar.terminal,
                if bar.state.query.is_empty() { "-".to_string() } else { bar.state.query.replace(' ', "_") },
                if bar.state.count_text().is_empty() {
                    "-".to_string()
                } else {
                    bar.state.count_text().replace(' ', "_")
                },
                bar.matches.len(),
                selection.replace(' ', "_"),
            )
        });
        lines.push(find.unwrap_or_else(|| "term-find open=n".to_string()));
        if let Some(pane) = self.workspace.file_pane() {
            for (index, label) in paddock_core::file_pane::tab_labels(pane).into_iter().enumerate() {
                lines.push(format!("file-tab-label {} {label}", index + 1));
            }
        }
        lines.extend(self.editing_summary(cx));
        lines
    }

    /// 이름 바꾸기 창 입력칸의 요약: `rename-input=<글자> rename-selected=<처음 골라 둔 글자> rename-check=valid|empty|invalid`. 창이 없으면 `rename-input=-`.
    fn rename_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        match (&self.overlay, self.rename_input.as_ref()) {
            (crate::window_view::Overlay::Rename(_), Some(input)) => {
                let state = input.read(cx);
                let value = state.value().to_string();
                let range = state.selected_range();
                let selected = value.get(range).unwrap_or("").to_string();
                let check = match paddock_core::entry_name::validate_entry_name(&value) {
                    paddock_core::entry_name::EntryNameCheck::Valid => "valid",
                    paddock_core::entry_name::EntryNameCheck::Empty => "empty",
                    paddock_core::entry_name::EntryNameCheck::Invalid(_) => "invalid",
                };
                format!("rename-input={value} rename-selected={selected} rename-check={check}")
            }
            _ => "rename-input=-".to_string(),
        }
    }

    /// 동기화 동작의 `act` 한 줄을 실행한다. 이 파일이 아는 명령이면 true다.
    ///
    /// | 명령                              | 하는 일                                                         |
    /// |-----------------------------------|-----------------------------------------------------------------|
    /// | `dblclick terminal N`             | N번째 탭의 터미널 행을 더블클릭한다(선택 + 이름 바꾸기 창)      |
    /// | `dragrow N M before\|after`       | N번째 터미널 행을 M번째 터미널 행의 앞·뒤에 끌어 놓는다          |
    /// | `showbeside N`                    | N번째 탭의 터미널을 `Show to the Side`로 옮긴다                  |
    /// | `paneadd P`                       | P번째(1부터) 칸의 ＋를 누른 것과 같다                            |
    /// | `tabbarfocus`                     | 탭 줄 빈 자리를 누른 것과 같다                                   |
    /// | `termfind 글`·`termfindnext`·`termfindprev` | 터미널 찾기 띠를 열고 질의를 넣는다 / 다음·이전 일치 |
    /// | `clipboard`                       | 클립보드를 읽어 종류(`text`·`files:N`·`image:<형식>`·`none`)를 상태 글 `clipboard=`에 남긴다(바꾸지 않는다) |
    /// | `selectrow N`                     | 파일 목록 N번째 행을 선택하고 목록이 키를 받게 한다 |
    /// | `filesdrop N 경로\|경로…`         | 탐색기에서 파일을 N번째 행(폴더면 그 안, 파일이면 부모)에 끌어 놓은 것과 같다. N이 0이면 빈 자리 |
    /// | `filesrename N 새이름`            | N번째 행의 이름 바꾸기 창을 열어 이름을 넣고 확인한다 |
    /// | `filespaste`·`termpaste`          | 실제 클립보드를 붙여 넣는다(사용자 클립보드를 읽으므로 야간에만) |
    /// | `termdrop 경로\|경로…`            | 선택한 터미널에 파일을 끌어 놓은 것과 같다 |
    pub(crate) fn sync_action(
        &mut self,
        parts: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let number = |index: usize| parts.get(index).and_then(|part| part.parse::<usize>().ok());
        let terminal_at = |this: &Self, n: Option<usize>| {
            n.and_then(|n| n.checked_sub(1)).and_then(|index| this.workspace.tabs().get(index)).map(|tab| tab.id)
        };
        let position = gpui_kit::point(
            gpui_kit::px(crate::automation::menu_anchor::X),
            gpui_kit::px(crate::automation::menu_anchor::Y),
        );
        let mut handled = true;
        match parts {
            ["dblclick", "terminal", _] => {
                if let Some(id) = terminal_at(self, number(2)) {
                    self.click_row(RowTarget::Terminal(id), 1, window, cx);
                    self.click_row(RowTarget::Terminal(id), 2, window, cx);
                }
            }
            ["dragrow", _, _, which] => {
                let moved = terminal_at(self, number(1));
                let target = terminal_at(self, number(2));
                if let (Some(moved), Some(target)) = (moved, target) {
                    let position = if *which == "after" { DropPosition::After } else { DropPosition::Before };
                    self.reorder_terminal(moved, target, position, cx);
                }
            }
            ["showbeside", _] => {
                if let Some(id) = terminal_at(self, number(1)) {
                    self.show_beside_terminal(id, window, cx);
                }
            }
            ["paneadd", _] => {
                let current = self.workspace.view_layout().and_then(|layout| {
                    let boxes = layout.pane_boxes(1.0, 1.0);
                    number(1)
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|index| boxes.get(index))
                        .and_then(|pane| pane.tabs.get(pane.selected.min(pane.tabs.len().saturating_sub(1))).copied())
                });
                if let Some(id) = current {
                    self.select_tab(id, window, cx);
                    let place = self.workspace.new_place();
                    self.open_new_terminal_menu(place, position, false, window, cx);
                }
            }
            ["tabbarfocus"] => self.focus_tab_bar(cx),
            ["clipboard"] => {
                let parts =
                    cx.read_from_clipboard().map(|item| crate::files::clipboard::parts_of(&item)).unwrap_or_default();
                let kind = match incoming_of(&parts) {
                    Some(Incoming::Paths(paths)) => format!("files:{}", paths.len()),
                    Some(Incoming::Image { kind, .. }) => format!("image:{kind:?}").to_lowercase(),
                    None if parts.iter().any(|part| matches!(part, ClipboardPart::Text)) => "text".to_string(),
                    None => "none".to_string(),
                };
                crate::automation::note_clipboard(kind);
            }
            ["selectrow", _] => {
                let row = number(1).and_then(|n| n.checked_sub(1)).and_then(|index| self.files.rows().get(index));
                if let Some(path) = row.map(|row| row.path.clone()) {
                    self.files.select_row(&path);
                }
            }
            ["filespaste"] => {
                let row = self.files.focused_entry();
                self.paste_into_files(row, window, cx);
            }
            ["filesdrop", row, paths @ ..] => {
                let target = row
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.files.rows().get(index))
                    .map(|row| (row.path.clone(), row.is_dir));
                self.drop_paths_into_files(paths_of(paths), target, window, cx);
            }
            ["filesrename", _, name @ ..] => {
                let target = number(1)
                    .and_then(|n| n.checked_sub(1))
                    .and_then(|index| self.files.rows().get(index))
                    .map(|row| (row.path.clone(), row.is_dir));
                if let Some((path, is_dir)) = target {
                    self.files.select_row(&path);
                    self.begin_rename(RenameTarget::Entry { path, is_dir }, window, cx);
                    if let Some(input) = self.rename_input.clone() {
                        input.update(cx, |state, cx| state.set_value(name.join(" "), window, cx));
                        self.commit_rename(window, cx);
                    }
                }
            }
            ["termdrop", paths @ ..] => {
                let view =
                    self.workspace.selected().and_then(|id| self.workspace.tab(id)).map(|tab| tab.content.view.clone());
                if let Some(view) = view {
                    view.update(cx, |view, cx| view.paste_paths(&paths_of(paths), cx));
                }
            }
            ["termpaste"] => {
                let view =
                    self.workspace.selected().and_then(|id| self.workspace.tab(id)).map(|tab| tab.content.view.clone());
                if let Some(view) = view {
                    view.update(cx, |view, cx| view.paste_clipboard(cx));
                }
            }
            ["termfind", query @ ..] => {
                self.open_terminal_find(window, cx);
                if let Some(input) = self.find.as_ref().map(|bar| bar.input.clone()) {
                    input.update(cx, |state, cx| state.set_value(query.join(" "), window, cx));
                    // 값을 코드로 넣으면 입력 사건이 오지 않으므로 사람이 친 것처럼 직접 다시 찾는다.
                    self.run_find(window, cx);
                }
            }
            ["termfindnext"] => self.find_step(true, cx),
            ["termfindprev"] => self.find_step(false, cx),
            ["quick", "copyonselection"] => self.quick_toggle_copy_on_selection(window, cx),
            _ => handled = false,
        }
        handled
    }
}
