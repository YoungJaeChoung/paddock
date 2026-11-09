//! 자동 점검용: 편집 흐름(저장·저장 여부 묻기·디스크 변경 띠·줄로 가기)의 대본 명령(`act`)과 창 상태 글 줄.
//! `automation` 기능으로 빌드한 개발용 실행 파일에만 들어간다.
use gpui_kit::{Context, Window};

use crate::window_view::{Overlay, WorkWindow};

impl WorkWindow {
    /// 창 상태 글에 붙는 편집 줄: `unsaved-files=N`·`status-save=unsaved|saved|-`·`disk-changed-files=N`·`goto-input=<글자>|-`.
    pub(crate) fn editing_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        let status = match self.save_status() {
            Some(paddock_core::unsaved::SaveStatus::Unsaved) => "unsaved",
            Some(paddock_core::unsaved::SaveStatus::Saved) => "saved",
            None => "-",
        };
        let mut lines = vec![
            format!("unsaved-files={}", self.unsaved.len()),
            format!("disk-changed-files={}", self.disk_changed.len()),
            format!("status-save={status}"),
            format!(
                "goto-input={}",
                self.goto_input.as_ref().map_or("-".to_string(), |input| input.read(cx).value().to_string())
            ),
        ];
        lines.extend(self.review_notes_summary());
        lines.extend(self.quick_open_summary(cx));
        lines.push(self.replace_summary(cx));
        lines
    }

    /// `file-view` 줄에 붙는 편집 항목: ` unsaved=yes|no disk-changed=yes|no find-open=yes|no`(편집기가 없는 탭은 find-open=no).
    pub(crate) fn editing_view_suffix(
        &self,
        path: &str,
        cx: &gpui_kit::App,
    ) -> String {
        let find_open = self.text_body_of(path).is_some_and(|body| body.editor.read(cx).search_session().is_active());
        format!(
            " unsaved={} disk-changed={} find-open={}",
            if self.unsaved.contains(path) { "yes" } else { "no" },
            if self.disk_changed.contains(path) { "yes" } else { "no" },
            if find_open { "yes" } else { "no" }
        )
    }

    /// 편집 흐름의 `act` 한 줄을 실행한다. 이 파일이 아는 명령이면 true다.
    ///
    /// | 명령                               | 하는 일                                                                    |
    /// |------------------------------------|----------------------------------------------------------------------------|
    /// | `save`                             | 상태 줄 `Save` 단추를 누른 것과 같다(Ctrl+S와 같은 길)                     |
    /// | `savechoice save\|dont\|cancel`    | 떠 있는 저장 묻기(`close-file`·`quit-unsaved`·`overwrite-file`)의 단추를 고른다 |
    /// | `reload`·`keepmine`                | 디스크 변경 띠의 `Reload`·`Keep mine`을 누른다(지금 파일 탭)               |
    /// | `findreplace 글`                    | 찾기 패널의 `Replace All` 단추를 누른 것과 같다(패널의 질의에 든 모든 일치를 `글`로 바꾼다) |
    /// | `selectlines A B`                  | 지금 파일 탭에서 A~B번째 줄(끝 줄 포함)을 고른다(변경 비교면 새 쪽 줄) |
    /// | `note 글`                          | Ctrl+Alt+M로 창을 열고 글을 넣어 확인한 것과 같다                          |
    /// | `sendnotes`·`clearnotes`           | 우클릭 메뉴의 `Send Review Notes to Agent`·`Clear Review Notes`            |
    /// | `editormenu`·`pickagent N`         | 편집기 우클릭 메뉴를 연다 / 열린 에이전트 고르기 메뉴의 N번째를 고른다     |
    /// | `gotoline N`                       | Ctrl+G로 창을 열고 N을 넣어 확인한 것과 같다                               |
    pub(crate) fn editing_action(
        &mut self,
        parts: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut handled = true;
        match parts {
            ["save"] => {
                self.save_active_file(cx);
            }
            ["savechoice", choice] => match (self.overlay.clone(), *choice) {
                (Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_), "save") => self.close_file_saving(window, cx),
                (Overlay::CloseFile(_) | Overlay::CloseDetachedFile(_), "dont") => {
                    self.close_file_discarding(window, cx)
                }
                (Overlay::QuitUnsaved, "save") => self.quit_saving_all(window, cx),
                (Overlay::QuitUnsaved, "dont") => self.quit_discarding(window, cx),
                (Overlay::OverwriteFile(path), "save") => self.save_overwriting(&path, window, cx),
                (_, "cancel") => self.cancel_overlay(window, cx),
                _ => {}
            },
            ["reload"] => {
                if let Some(path) = self.active_file_path() {
                    self.reload_from_disk(&path, window, cx);
                }
            }
            ["keepmine"] => {
                if let Some(path) = self.active_file_path() {
                    self.keep_mine(&path, cx);
                }
            }
            ["findreplace", replacement @ ..] => {
                let editor =
                    self.active_file_path().and_then(|path| self.text_body_of(&path).map(|body| body.editor.clone()));
                if let Some(editor) = editor {
                    let text = replacement.join(" ");
                    let n_replaced = editor.update(cx, |state, cx| state.replace_all_search_matches(&text, window, cx));
                    crate::automation::event(&format!("findreplace: {n_replaced} replaced"));
                }
            }
            ["selectlines", first, last] => {
                if let (Ok(first), Ok(last)) = (first.parse::<usize>(), last.parse::<usize>()) {
                    self.select_lines(first, last, cx);
                }
            }
            ["note", text @ ..] => {
                if self.begin_review_note(window, cx)
                    && let Some(input) = self.note_input.clone()
                {
                    input.update(cx, |state, cx| state.set_value(text.join(" "), window, cx));
                    self.commit_review_note(window, cx);
                }
            }
            ["sendnotes"] => {
                let at = gpui_kit::point(
                    gpui_kit::px(crate::automation::menu_anchor::X),
                    gpui_kit::px(crate::automation::menu_anchor::Y),
                );
                self.send_review_notes(at, window, cx);
            }
            ["clearnotes"] => self.clear_review_notes(cx),
            ["editormenu"] => {
                let at = gpui_kit::point(
                    gpui_kit::px(crate::automation::menu_anchor::X),
                    gpui_kit::px(crate::automation::menu_anchor::Y),
                );
                self.open_editor_menu(at, window, cx);
            }
            ["pickagent", n] => {
                let picked = match self.menu.as_ref().map(|menu| &menu.kind) {
                    Some(crate::work::menu_model::MenuKind::PickAgent(agents)) => n
                        .parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|index| agents.get(index))
                        .map(|(id, _)| *id),
                    _ => None,
                };
                if let Some(id) = picked {
                    self.menu = None;
                    self.send_notes_to(id, window, cx);
                }
            }
            ["gotoline", line] => {
                if self.begin_go_to_line(window, cx)
                    && let Some(input) = self.goto_input.clone()
                {
                    input.update(cx, |state, cx| state.set_value((*line).to_string(), window, cx));
                    self.commit_go_to_line(window, cx);
                }
            }
            _ => handled = false,
        }
        handled
    }
}
