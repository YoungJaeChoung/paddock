//! 검토 메모: 편집기·변경 비교에서 고른 줄에 메모를 남기고, 모은 메모를 에이전트 터미널에 한 번에 붙여 넣는다.
//!
//! 메모는 이 창 안에만 모으고(`WorkWindow::notes`, 파일로 남기지 않는다) 붙여 넣기만 한다 — Enter는 누르지 않아 사용자가 확인·보충한 뒤 보낸다. 보내면 비운다.
//! 글 만들기·줄 범위·보낼 터미널 정하기는 core의 `review_notes`가 하고, 여기서는 창 상태(선택·묻기 창·메뉴·터미널)에 연결한다.
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::{AppContext, Context, Window};
use paddock_core::file_kind::FileKind;
use paddock_core::review_notes::{
    CLEARED, NO_AGENT, NO_EDITOR, Note, PASTED, SendTarget, added_text, format_notes, full_text, limits, line_range_of,
    relative_path, send_target,
};

use crate::files::pane::FileContent;
use crate::window_view::{Overlay, WorkWindow};
use crate::work::menu_model::MenuKind;

/// 고른 줄: 메모에 담을 파일 경로(절대)·줄 범위·시작 줄 내용.
struct Selected {
    file: String,
    start_line: usize,
    end_line: usize,
    line_text: String,
}

/// `value`의 `offset`(바이트)가 놓인 줄(0부터)과 줄 안 열(바이트, 0부터).
fn line_and_column(
    value: &str,
    offset: usize,
) -> (usize, usize) {
    let head = &value[..offset.min(value.len())];
    let line = head.matches('\n').count();
    let column = head.len() - head.rfind('\n').map_or(0, |at| at + 1);
    (line, column)
}

/// `value`에서 `line`번째(0부터) 줄이 시작하는 바이트 위치. 줄이 모자라면 글 끝이다.
#[cfg(feature = "automation")]
fn line_start(
    value: &str,
    line: usize,
) -> usize {
    let mut offset = 0;
    for _ in 0..line {
        match value[offset..].find('\n') {
            Some(at) => offset += at + 1,
            None => {
                offset = value.len();
                break;
            }
        }
    }
    offset
}

impl WorkWindow {
    /// 지금 파일 탭에서 고른 줄. 텍스트·Markdown 원문 편집기면 고른 글자의 줄이고, 변경 비교면 고른 줄(새 쪽)이다. 고른 곳이 없으면 None이다.
    fn review_selection(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<Selected> {
        let tab = self.active_file_tab();
        tab.and_then(|tab| match (tab.kind, self.file_views.get(&tab.key())) {
            (FileKind::Diff { .. }, Some(FileContent::Diff(body))) => {
                let state = body.state.read(cx);
                // 새 쪽(Modified) 줄만 메모가 가리킬 수 있다. 지워진 쪽(Original)이 든 선택은 줄 번호가 현재 파일의 줄이 아니므로 받지 않는다.
                let modified = gpui_kit::component::diff::DiffSide::Modified;
                state.selected_lines().filter(|range| range.side() == modified && range.end_side() == modified).map(
                    |range| {
                        let (start, end) = (range.start().min(range.end()), range.start().max(range.end()));
                        // 고른 글(번호·표식 없이 소스만)의 첫 줄이 시작 줄 내용이다.
                        let line_text = state.selected_text(cx).lines().next().unwrap_or_default().to_string();
                        Selected { file: tab.path.clone(), start_line: start, end_line: end, line_text }
                    },
                )
            }
            (FileKind::Diff { .. }, _) => None,
            _ => self.text_body_of(&tab.path).map(|body| {
                let editor = body.editor.read(cx);
                let value = editor.value().to_string();
                let range = editor.selected_range();
                let (start, end) = (line_and_column(&value, range.start), line_and_column(&value, range.end));
                let (start_line, end_line) = line_range_of(start, end);
                let line_text = value.lines().nth(start_line - 1).unwrap_or_default().to_string();
                Selected { file: tab.path.clone(), start_line, end_line, line_text }
            }),
        })
    }

    /// Ctrl+Alt+M·메뉴 `Add Review Note…`: 고른 줄에 대한 메모를 입력받는 창을 연다. 열었으면 true다(고른 곳이 없거나 메모가 가득 차면 상태 줄에 알리고 false).
    pub(crate) fn begin_review_note(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let selected = self.review_selection(cx);
        let full = self.notes.len() >= limits::MAX_NOTES;
        let opens = selected.is_some() && !full;
        match (selected, full) {
            (None, _) => self.status = Some(NO_EDITOR.to_string()),
            (Some(_), true) => self.status = Some(full_text()),
            (Some(selected), false) => {
                self.settle_composition(window, cx);
                let input =
                    cx.new(|cx| InputState::new(window, cx).placeholder(paddock_core::review_notes::INPUT_PLACEHOLDER));
                cx.subscribe_in(
                    &input,
                    window,
                    |this, input, event: &gpui_kit::component::input::InputEvent, window, cx| match event {
                        gpui_kit::component::input::InputEvent::PressEnter { .. }
                            if this.note_input.as_ref() == Some(input) =>
                        {
                            this.commit_review_note(window, cx)
                        }
                        gpui_kit::component::input::InputEvent::Change => cx.notify(),
                        _ => {}
                    },
                )
                .detach();
                self.note_input = Some(input.clone());
                self.open_overlay(
                    Overlay::ReviewNote {
                        file: selected.file,
                        start_line: selected.start_line,
                        end_line: selected.end_line,
                        line_text: selected.line_text,
                    },
                    window,
                    cx,
                );
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        cx.notify();
        opens
    }

    /// 메모 입력창에 적은 글(앞뒤 공백을 뗀 것). 비었으면 None이다.
    pub(crate) fn review_note_text(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<String> {
        self.note_input.as_ref().map(|input| input.read(cx).value().trim().to_string()).filter(|text| !text.is_empty())
    }

    /// 메모 입력창의 OK·Enter: 적은 글을 메모로 더한다. 글이 비었으면 아무것도 하지 않는다.
    pub(crate) fn commit_review_note(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.review_note_text(cx);
        if let (Some(text), Overlay::ReviewNote { file, start_line, end_line, line_text }) =
            (text, self.overlay.clone())
        {
            let roots: Vec<String> = self.workspace.work().folders.iter().map(|folder| folder.key.clone()).collect();
            self.notes.push(Note { file: relative_path(&file, &roots), start_line, end_line, line_text, text });
            self.note_input = None;
            self.overlay = Overlay::None;
            self.status = Some(added_text(self.notes.len()));
            if !self.end_detached_file_context(cx) {
                self.focus_selected(window, cx);
            }
            cx.notify();
        }
    }

    /// 지금 에이전트가 실행 중인 터미널들(탭 순서).
    fn agent_terminals(&self) -> Vec<(u32, String)> {
        self.workspace
            .tabs()
            .iter()
            .filter(|tab| {
                tab.content
                    .session
                    .grid
                    .shared
                    .state
                    .lock()
                    .ok()
                    .is_some_and(|state| state.as_ref().is_some_and(|state| state.is_agent))
            })
            .map(|tab| (tab.id, self.workspace.name(tab.id).to_string()))
            .collect()
    }

    /// 메뉴 `Send Review Notes to Agent`: 모은 메모를 에이전트 터미널에 붙여 넣는다. 보낼 터미널이 여럿이면 고르게 하고, 없으면 메모를 그대로 두고 알린다.
    pub(crate) fn send_review_notes(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.notes.is_empty() {
            let agents = self.agent_terminals();
            let ids: Vec<u32> = agents.iter().map(|(id, _)| *id).collect();
            match send_target(self.workspace.selected(), &ids) {
                SendTarget::One(id) => self.send_notes_to(id, window, cx),
                SendTarget::Pick(_) => self.open_menu(MenuKind::PickAgent(agents), position, window, cx),
                SendTarget::None => self.status = Some(NO_AGENT.to_string()),
            }
        }
        cx.notify();
    }

    /// 에이전트 터미널 `id`에 메모 글을 붙여 넣고(Enter는 누르지 않는다) 그 터미널을 고른 뒤 메모를 비운다.
    pub(crate) fn send_notes_to(
        &mut self,
        id: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.workspace.tab(id).map(|tab| tab.content.view.clone());
        if let Some(view) = view {
            let text = format_notes(&self.notes);
            view.update(cx, |view, cx| view.paste_text(&text, cx));
            self.notes.clear();
            self.select_tab(id, window, cx);
            self.status = Some(PASTED.to_string());
        }
        cx.notify();
    }

    /// 메뉴 `Clear Review Notes`.
    pub(crate) fn clear_review_notes(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.notes.clear();
        self.status = Some(CLEARED.to_string());
        cx.notify();
    }

    /// 편집기 우클릭 메뉴를 `position`에 연다(부품의 기본 메뉴를 대신한다).
    pub(crate) fn open_editor_menu(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 파일 분리 창에서 누른 우클릭은 메뉴를 열지 않는다: 메뉴는 본 창이 그리는데 `position`은 분리 창 안의 자리라 본 창 엉뚱한 곳에 열린다.
        // (분리 창의 잘라내기·복사·붙이기·찾기는 키로 쓴다.)
        if window.window_handle() == self.main_window {
            self.open_menu(MenuKind::Editor { n_notes: self.notes.len() }, position, window, cx);
        }
    }

    /// 편집기 메뉴의 `Cut`·`Copy`·`Paste`: 편집기에 포커스를 주고 그 동작을 보낸다.
    pub(crate) fn editor_clipboard_action(
        &mut self,
        action: Box<dyn gpui_kit::Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_selected(window, cx);
        window.dispatch_action(action, cx);
    }

    /// 대본이 쓰는 줄 고르기: 지금 파일 탭의 `first`~`last`번째 줄(1부터, 끝 줄 포함)을 고른다. 변경 비교면 새 쪽 줄을 고른다.
    #[cfg(feature = "automation")]
    pub(crate) fn select_lines(
        &mut self,
        first: usize,
        last: usize,
        cx: &mut Context<Self>,
    ) {
        let tab = self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).cloned();
        if let Some(tab) = tab {
            match (tab.kind, self.file_views.get(&tab.key())) {
                (FileKind::Diff { .. }, Some(FileContent::Diff(body))) => {
                    let path = body.state.read(cx).files().first().map(|file| file.path().clone());
                    if let Some(path) = path {
                        let range = gpui_kit::component::diff::DiffLineRange::new(
                            path,
                            gpui_kit::component::diff::DiffSide::Modified,
                            first,
                            last,
                        );
                        body.state.clone().update(cx, |state, cx| state.set_selected_lines(Some(range), cx));
                    }
                }
                _ => {
                    let editor = self.text_body_of(&tab.path).map(|body| body.editor.clone());
                    if let Some(editor) = editor {
                        let value = editor.read(cx).value().to_string();
                        let start = line_start(&value, first.saturating_sub(1));
                        let end = line_start(&value, last).saturating_sub(usize::from(last < value.lines().count()));
                        editor.update(cx, |state, cx| state.set_selected_range(start..end.max(start), cx));
                    }
                }
            }
        }
    }

    /// 창 상태 글에 붙는 검토 메모 줄: `review-notes=N`과 메모마다 `review-note 번호 파일:줄 글`.
    #[cfg(feature = "automation")]
    pub(crate) fn review_notes_summary(&self) -> Vec<String> {
        let mut lines = vec![format!("review-notes={}", self.notes.len())];
        for (index, note) in self.notes.iter().enumerate() {
            let range = if note.end_line > note.start_line {
                format!("{}-{}", note.start_line, note.end_line)
            } else {
                note.start_line.to_string()
            };
            lines.push(format!("review-note {} {}:{range} {}", index + 1, note.file, note.text.replace(' ', "_")));
        }
        lines
    }

    /// 상태 줄 오른쪽에 보일 메모 개수 글(`Review notes: N`). 메모가 없으면 None이다.
    pub(crate) fn review_notes_label(&self) -> Option<String> {
        (!self.notes.is_empty()).then(|| paddock_core::review_notes::status_text(self.notes.len()))
    }
}

/// 편집기의 우클릭을 앱 메뉴로 보낸다(부품은 우클릭 때 이 처리기를 부른다).
pub fn route_context_menu(
    editor: &gpui_kit::Entity<gpui_kit::component::input::EditorState>,
    cx: &mut Context<WorkWindow>,
) {
    let window_view = cx.weak_entity();
    editor.update(cx, |state, _| {
        state.on_context_menu(Rc::new(move |_menu, _capabilities, position, window, cx| {
            let _ = window_view.update(cx, |this, cx| this.open_editor_menu(position, window, cx));
        }));
    });
}
