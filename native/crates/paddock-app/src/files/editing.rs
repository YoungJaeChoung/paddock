//! 파일 편집 흐름: 고친 글자를 저장하고, 저장하지 않은 채 닫거나 종료할 때 묻고, 디스크가 바뀌었을 때 띠로 고르게 하고, 줄로 간다.
//!
//! 편집기 글자는 `TextBody`(텍스트 탭 또는 Markdown 원문)가 쥐고, 여기서는 "저장하지 않은 파일" 집합(`unsaved`)과 디스크 변경 파일(`disk_changed`)만 움직인다.
//! 디스크에 쓰는 일은 `WriteService`의 `Save` 명령 하나로 들어가고, 결과는 `pump_writes`가 `finish_save`로 넘긴다.
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext, Context, Window};
use paddock_core::file_kind::{FileKind, file_name_of};
use paddock_core::file_pane::FileTab;
use paddock_core::text_file::encode_for_save;
use paddock_core::unsaved::{
    RENAME_PENDING_TEXT, SAVED_SHOWN_SECONDS, changed_while_saving_text, quit_save_blocked_text, save_failed_text,
    saved_before_close_text, stays_unsaved_after_save,
};

use crate::files::open::file_stamp;
use crate::files::ops::SaveOutcome;
use crate::files::pane::FileContent;
use crate::files::text_view::{TextBody, scroll_to_line};
use crate::files::write_service::WriteCommand;
use crate::window_view::{Overlay, WorkWindow};

/// 상태 줄이 `Saved`를 보이는 시간.
const SAVED_SHOWN: Duration = Duration::from_secs(SAVED_SHOWN_SECONDS);

impl WorkWindow {
    /// 편집기(`editor`)의 글자가 바뀌었다(사용자가 고쳤다). 그 편집기를 쥔 파일을 저장하지 않은 상태로 둔다. 되돌리기로 글자가 같아져도 저장하기 전에는 그대로다.
    pub(crate) fn note_edited(
        &mut self,
        editor: gpui_kit::EntityId,
        cx: &mut Context<Self>,
    ) {
        let path = self
            .file_views
            .keys()
            .find(|path| self.text_body_of(path).is_some_and(|body| body.editor.entity_id() == editor))
            .cloned();
        if let Some(path) = path {
            self.unsaved.mark(&path);
        }
        cx.notify();
    }

    /// 파일 열쇠(경로)의 편집기. 텍스트 탭이거나 Markdown 탭의 원문 보기(만들어져 있을 때)다.
    pub(crate) fn text_body_of(
        &self,
        path: &str,
    ) -> Option<&TextBody> {
        match self.file_views.get(path) {
            Some(FileContent::Text(body)) => Some(body),
            Some(FileContent::Markdown(body)) => body.source.as_ref(),
            _ => None,
        }
    }

    /// 지금 키를 받는 파일 탭(파일 탭이 활성일 때만). 분리 창의 파일 단축키가 묻기 창을 열어 둔 동안은 그 분리 창의 파일 탭이다
    /// (분리 창이 활성인 파일은 늘 그 파일이다).
    pub(crate) fn active_file_tab(&self) -> Option<&FileTab> {
        match &self.detached_file_context {
            Some(key) => self.workspace.detached_file(key).map(|detached| &detached.tab),
            None => {
                self.workspace.file_pane().and_then(|pane| pane.tabs.get(pane.selected)).filter(|_| self.file_active())
            }
        }
    }

    /// 지금 키를 받는 파일 탭의 경로(파일 탭이 활성일 때만). 변경 비교 탭은 파일 내용이 아니라 `git diff`라서 경로가 없다(같은 파일의 본문 탭과 헷갈리지 않게).
    pub(crate) fn active_file_path(&self) -> Option<String> {
        self.active_file_tab().filter(|tab| !matches!(tab.kind, FileKind::Diff { .. })).map(|tab| tab.path.clone())
    }

    /// 분리 창의 파일 단축키가 연 묻기 창이 끝났다: 그 맥락을 거두고, 있었으면 그 분리 창을 앞으로 가져와 키 입력을 돌려준다.
    /// 맥락이 없었으면 false다(호출자가 본 창의 선택에 포커스를 준다).
    pub(crate) fn end_detached_file_context(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.detached_file_context.take() {
            Some(key) => {
                self.activate_detached_file(&key, cx);
                true
            }
            None => false,
        }
    }

    /// Ctrl+S·상태 줄 `Save` 단추: 지금 파일 탭을 저장한다. 저장하지 않은 변경이 없거나 고칠 수 없는 파일이면 아무것도 하지 않는다.
    /// 저장을 시작했으면 true다.
    pub(crate) fn save_active_file(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        let started = self.active_file_path().is_some_and(|path| self.save_file_path(&path, false, cx));
        cx.notify();
        started
    }

    /// `path`의 편집 내용을 저장 명령으로 보낸다. `force`는 디스크가 바뀐 줄 알면서 덮어쓰겠다는 뜻이다. 저장하지 않은 변경이 없거나
    /// 이미 저장 중이거나 고칠 수 없는 파일이면 보내지 않고 false다.
    pub(crate) fn save_file_path(
        &mut self,
        path: &str,
        force: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let renaming = self.renaming.iter().any(|from| paddock_core::paths::rebased(from, from, path).is_some());
        if renaming {
            self.status = Some(RENAME_PENDING_TEXT.to_string());
        }
        let prepared = self
            .text_body_of(path)
            .filter(|_| !renaming)
            .filter(|body| body.editable)
            .map(|body| {
                let text = body.editor.read(cx).value().to_string();
                (encode_for_save(&text, &body.encoded), text)
            })
            .filter(|_| self.unsaved.contains(path) && !self.saving.contains(path));
        let started = prepared.is_some();
        if let Some((bytes, text)) = prepared {
            let expected = self.file_stamps.get(path).copied().flatten();
            self.saving.insert(path.to_string());
            self.writes.submit(WriteCommand::Save { path: path.to_string(), bytes, text, expected, force });
        }
        started
    }

    /// 저장하지 않은 파일을 모두 저장 명령으로 보낸다. 보낸 파일의 경로를 돌려준다.
    pub(crate) fn save_all_unsaved(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let paths: Vec<String> = self.unsaved.paths().cloned().collect();
        paths.into_iter().filter(|path| self.save_file_path(path, false, cx)).collect()
    }

    /// 저장 명령의 결과를 받는다. 썼으면 저장하지 않은 표시를 지우고(저장하는 동안 더 고쳤으면 남긴다), 디스크가 바뀌어 있었으면 덮어쓸지 묻고(여럿이면 차례로),
    /// 실패하면 상태 줄에 알린다. 저장 뒤에 탭을 닫거나 창을 닫기로 한 파일은 여기서 이어서 한다.
    pub(crate) fn finish_save(
        &mut self,
        path: &str,
        text: &str,
        outcome: SaveOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.saving.remove(path);
        let name = file_name_of(path);
        match outcome {
            SaveOutcome::Written(stamp) => {
                self.file_stamps.insert(path.to_string(), stamp);
                self.own_write_generation.insert(path.to_string(), self.open_files_check_generation);
                self.disk_changed.remove(path);
                let edited_meanwhile =
                    self.text_body_of(path).is_some_and(|body| *body.editor.read(cx).value() != *text);
                if !stays_unsaved_after_save(true, edited_meanwhile) {
                    self.unsaved.clear(path);
                }
                self.saved_flash = Some((path.to_string(), Instant::now()));
                self.status = self.closed_while_saving.remove(path).then(|| saved_before_close_text(&name));
                self.git.note(crate::git::gate::Trigger::Change, Instant::now());
                if let Some(FileContent::Markdown(body)) = self.file_views.get_mut(path) {
                    body.show_saved(text, cx);
                }
                // 2초 뒤 `Saved`를 지우려고 다시 그린다.
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor().timer(SAVED_SHOWN + Duration::from_millis(100)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
                self.continue_after_save(path, true, window, cx);
            }
            SaveOutcome::Conflict => {
                // 탭 닫기·종료는 그대로 기다린다(덮어쓰면 이어 가고 취소하면 멈춘다). 묻기 창이 이미 떠 있으면 차례를 기다리고, 그동안 띠가 알린다.
                self.disk_changed.insert(path.to_string());
                if !self.overwrite_queue.iter().any(|queued| queued == path) {
                    self.overwrite_queue.push_back(path.to_string());
                }
                self.show_next_overwrite(window, cx);
            }
            SaveOutcome::Failed(reason) => {
                self.status = Some(save_failed_text(&name, &reason));
                self.continue_after_save(path, false, window, cx);
            }
        }
        cx.notify();
    }

    /// 덮어쓰기 묻기를 기다리는 파일이 있고 다른 묻기 창이 없으면 다음 파일의 묻기 창을 연다.
    pub(crate) fn show_next_overwrite(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.overlay == Overlay::None
            && let Some(path) = self.overwrite_queue.pop_front()
        {
            // 파일 분리 창의 파일이면 묻기 창은 본 창에 열리므로 본 창을 앞으로 가져오고, 끝나면 그 분리 창으로 돌아가게 맥락을 둔다.
            self.detached_file_context = self.detached_file_key_of(&path);
            if self.detached_file_context.is_some() {
                self.activate_main(window);
            }
            self.open_overlay(Overlay::OverwriteFile(path), window, cx);
        }
    }

    /// 저장 결과가 나온 뒤 이어서 할 일: `Save`를 골라 닫으려던 탭은 저장됐을 때 닫고(그 탭이 속한 묶음에서 찾는다), `Save all`로 종료하려던 창은 모두 저장됐을 때 닫는다.
    /// 하나라도 저장하지 못하면 둘 다 멈추고 파일은 열린 채로 둔다. 모두 저장했는데 저장하는 동안 더 고친 파일이 남았으면 종료 묻기를 다시 띄운다.
    fn continue_after_save(
        &mut self,
        path: &str,
        saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing = self.close_after_save.remove(path);
        if let Some(group) = closing
            && saved
        {
            if self.unsaved.contains(path) {
                // 저장하는 동안 더 고쳤다: 그 편집을 잃지 않게 닫기 묻기를 다시 띄운다.
                let index = self.workspace.files().get(&group).and_then(|pane| {
                    pane.tabs.iter().position(|tab| tab.path == path && !matches!(tab.kind, FileKind::Diff { .. }))
                });
                if let (Some(index), true) = (index, self.workspace.view_key().as_ref() == Some(&group)) {
                    self.open_overlay(Overlay::CloseFile(index), window, cx);
                } else if let Some(key) = self.detached_file_key_of(path) {
                    self.open_overlay(Overlay::CloseDetachedFile(key), window, cx);
                }
            } else {
                self.close_file_tab_of(&group, path, window, cx);
            }
        }
        let quitting = self.quit_saving.remove(path);
        if quitting && !saved {
            self.quit_saving.clear();
        } else if quitting && self.quit_saving.is_empty() {
            if self.unsaved.is_empty() {
                self.quit_unsaved_resolved = true;
                if self.on_close_requested(window, cx) {
                    window.remove_window();
                }
            } else {
                self.status = Some(changed_while_saving_text(self.unsaved.len()));
                self.open_overlay(Overlay::QuitUnsaved, window, cx);
            }
        }
    }

    /// 묶음 `group`의 열린 파일 탭 중 `path`를 보여 주는 탭을 확인 없이 닫는다.
    fn close_file_tab_of(
        &mut self,
        group: &paddock_core::work_model::GroupKey,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.workspace.files().get(group).and_then(|pane| {
            pane.tabs.iter().position(|tab| tab.path == path && !matches!(tab.kind, FileKind::Diff { .. }))
        });
        if let Some(index) = index {
            self.close_file_tab_in(group.clone(), index, window, cx);
        } else if let Some(key) = self.detached_file_key_of(path) {
            // 파일 분리 창에 있던 탭이다.
            self.close_detached_file_now(&key, window, cx);
        }
    }

    /// 다른 창에 있는 본문 파일 탭 중 `path`를 보여 주는 탭의 열쇠.
    pub(crate) fn detached_file_key_of(
        &self,
        path: &str,
    ) -> Option<String> {
        self.workspace
            .detached_files()
            .iter()
            .find(|detached| detached.tab.path == path && !matches!(detached.tab.kind, FileKind::Diff { .. }))
            .map(|detached| detached.tab.key())
    }

    /// 같은 파일(`path`)을 보여 주는 열린 본문 탭의 수(모든 묶음). 둘 이상이면 한 편집기를 여러 탭이 나눠 쥔다.
    pub(crate) fn n_tabs_of(
        &self,
        path: &str,
    ) -> usize {
        self.workspace.file_tabs().filter(|tab| tab.path == path && !matches!(tab.kind, FileKind::Diff { .. })).count()
    }

    /// 파일 탭이 활성일 때만 앱이 가져가는 키(Ctrl+S 저장·Ctrl+G 줄로 가기)를 실행한다. 가져갔으면 true다.
    /// 터미널이 활성이거나 지금 파일 탭에 편집기가 없으면(이미지·PDF 등) false라서 키가 그대로 터미널·부품으로 간다.
    pub(crate) fn run_file_shortcut(
        &mut self,
        shortcut: Option<paddock_core::app_shortcut::FileShortcut>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use paddock_core::app_shortcut::FileShortcut;
        let has_editor = self.active_file_path().is_some_and(|path| self.text_body_of(&path).is_some());
        match shortcut {
            Some(FileShortcut::Save) if has_editor => {
                self.save_active_file(cx);
                true
            }
            Some(FileShortcut::GoToLine) if has_editor => self.begin_go_to_line(window, cx),
            // 파일 탭이 활성이면(편집기든 변경 비교든) 이 키는 앱이 가져간다. 고른 곳이 없으면 상태 줄에 알린다.
            Some(FileShortcut::AddReviewNote) if self.file_active() || self.detached_file_context.is_some() => {
                self.begin_review_note(window, cx);
                true
            }
            _ => false,
        }
    }

    /// 묶음·폴더(`key`)의 열린 파일 탭 중 저장하지 않은 것의 수.
    pub(crate) fn n_unsaved_in(
        &self,
        key: &paddock_core::work_model::GroupKey,
    ) -> usize {
        // 같은 파일이 다른 묶음에도 열려 있으면 그쪽 탭이 편집을 이어 쥐므로 이 묶음을 닫아도 잃지 않는다.
        let open_elsewhere = |path: &str| {
            self.workspace
                .files()
                .iter()
                .filter(|(other, _)| other != key)
                .flat_map(|(_, pane)| pane.tabs.iter())
                .any(|tab| tab.path == path && !matches!(tab.kind, FileKind::Diff { .. }))
        };
        self.workspace.files().get(key).map_or(0, |pane| {
            pane.tabs.iter().filter(|tab| self.unsaved.contains(&tab.key()) && !open_elsewhere(&tab.path)).count()
        })
    }

    /// 터미널 `id`를 닫으면 함께 닫히는 파일 탭(`files_closed_with`) 중 저장하지 않은 것의 수.
    pub(crate) fn n_unsaved_closed_with(
        &self,
        id: u32,
    ) -> usize {
        let work = self.workspace.work();
        match work.group_of(id) {
            Some(root) if work.group_members(root).len() == 1 => {
                self.n_unsaved_in(&paddock_core::work_model::GroupKey::Group(root))
            }
            _ => 0,
        }
    }

    /// 파일 탭 닫기 요청(× 단추·Ctrl+Shift+W·대본): 이 탭이 그 파일의 마지막 탭이고 저장하지 않은 변경이 있으면 저장 여부를 묻고, 아니면 바로 닫는다.
    /// 같은 파일이 다른 묶음에도 열려 있으면 편집기는 그쪽 탭이 이어 쥐므로 묻지 않는다.
    pub(crate) fn close_file_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asks = self
            .workspace
            .file_pane()
            .and_then(|pane| pane.tabs.get(index))
            .is_some_and(|tab| self.unsaved.contains(&tab.key()) && self.n_tabs_of(&tab.path) <= 1);
        if asks {
            self.open_overlay(Overlay::CloseFile(index), window, cx);
        } else {
            self.close_file_tab_now(index, window, cx);
        }
    }

    /// 닫기 묻기 창의 파일 경로(창이 떠 있고 그 탭이 아직 있을 때).
    pub(crate) fn close_prompt_path(&self) -> Option<String> {
        match &self.overlay {
            Overlay::CloseFile(index) => {
                self.workspace.file_pane().and_then(|pane| pane.tabs.get(*index)).map(|tab| tab.path.clone())
            }
            Overlay::CloseDetachedFile(key) => {
                self.workspace.detached_file(key).map(|detached| detached.tab.path.clone())
            }
            _ => None,
        }
    }

    /// 닫기 묻기에서 `Save`: 저장하고, 저장되면 탭을 닫는다(덮어쓰기 묻기를 거쳐 저장돼도 닫는다).
    pub(crate) fn close_file_saving(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.close_prompt_path();
        // 다른 창의 파일이면 그 파일이 돌아올 묶음이 "닫을 탭이 든 묶음"이다.
        let group = match &self.overlay {
            Overlay::CloseDetachedFile(key) => self.workspace.detached_file(key).map(|detached| detached.group.clone()),
            _ => self.workspace.view_key(),
        };
        self.overlay = Overlay::None;
        if let (Some(path), Some(group)) = (path, group) {
            if !self.unsaved.contains(&path) {
                // 묻기 창이 떠 있는 동안 저장이 끝났다: 저장할 것이 없으니 바로 닫는다.
                self.close_file_tab_of(&group, &path, window, cx);
            } else if self.saving.contains(&path) || self.save_file_path(&path, false, cx) {
                // 이미 저장 중이면 그 저장이 끝났을 때 닫는다(새 저장은 보내지 않는다).
                self.close_after_save.insert(path, group);
            } else {
                // 저장할 수 없는 파일(고칠 수 없음·이미 저장 중)은 닫지 않고 둔다.
                self.status = Some(save_failed_text(&file_name_of(&path), "it cannot be saved right now."));
            }
        }
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 닫기 묻기에서 `Don't Save`: 이 탭만 닫는다. 같은 파일을 보여 주는 다른 탭이 없을 때만 고친 내용을 버린다(있으면 그 탭이 편집을 이어 쥔다).
    pub(crate) fn close_file_discarding(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let detached = match &self.overlay {
            Overlay::CloseDetachedFile(key) => self.workspace.detached_file(key).map(|detached| detached.tab.clone()),
            _ => None,
        };
        let target = match &self.overlay {
            Overlay::CloseFile(index) => self
                .workspace
                .file_pane()
                .and_then(|pane| pane.tabs.get(*index))
                .map(|tab| (Some(*index), tab.path.clone())),
            _ => detached.as_ref().map(|tab| (None, tab.path.clone())),
        };
        self.overlay = Overlay::None;
        if let Some((index, path)) = target {
            if self.n_tabs_of(&path) <= 1 {
                self.unsaved.clear(&path);
                self.disk_changed.remove(&path);
                if self.saving.contains(&path) {
                    self.closed_while_saving.insert(path.clone());
                }
            }
            match (index, detached) {
                (Some(index), _) => self.close_file_tab_now(index, window, cx),
                (None, Some(tab)) => self.close_detached_file_now(&tab.key(), window, cx),
                (None, None) => {}
            }
        }
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 종료 묻기(저장하지 않은 파일이 있을 때)에서 `Save all`: 모두 저장하고, 모두 저장되면 창을 닫는다. 이미 저장 중인 파일은 그 저장이 끝나기를 기다린다.
    /// 저장하는 동안 더 고친 파일이 남으면 종료 묻기를 다시 띄우고, 저장 명령을 하나도 보낼 수 없으면 창을 그대로 두며 이유를 상태 줄에 둔다.
    pub(crate) fn quit_saving_all(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        let mut waiting = self.save_all_unsaved(cx);
        let in_flight: Vec<String> = self.unsaved.paths().filter(|path| self.saving.contains(*path)).cloned().collect();
        for path in in_flight {
            if !waiting.contains(&path) {
                waiting.push(path);
            }
        }
        if waiting.is_empty() {
            self.status = Some(quit_save_blocked_text(self.unsaved.len()));
            self.focus_selected(window, cx);
        } else {
            self.quit_saving = waiting.into_iter().collect();
        }
        cx.notify();
    }

    /// 종료 묻기에서 `Discard`: 저장하지 않은 변경을 버리고 창 닫기를 이어 간다.
    pub(crate) fn quit_discarding(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        self.quit_unsaved_resolved = true;
        if self.on_close_requested(window, cx) {
            window.remove_window();
        }
        cx.notify();
    }

    /// 디스크 변경 띠의 `Reload`: 고친 내용을 버리고 디스크의 내용을 다시 읽는다.
    pub(crate) fn reload_from_disk(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.open_file_kind(path);
        self.unsaved.clear(path);
        self.disk_changed.remove(path);
        if let Some(kind) = kind {
            let ignore_nul = self.forced_text.contains(path);
            self.start_file_load(path.to_string(), kind, ignore_nul, window, cx);
        }
        cx.notify();
    }

    /// 디스크 변경 띠의 `Keep mine`: 고친 내용을 지키고, 디스크의 지금 모습을 새 기준으로 삼는다(다음 저장이 묻지 않고 덮어쓴다).
    pub(crate) fn keep_mine(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        self.disk_changed.remove(path);
        self.file_stamps.insert(path.to_string(), file_stamp(path));
        cx.notify();
    }

    /// 열린 파일 탭 중 `path`를 보여 주는 탭의 종류(변경 비교 탭은 제외).
    pub(crate) fn open_file_kind(
        &self,
        path: &str,
    ) -> Option<FileKind> {
        self.workspace
            .file_tabs()
            .find(|tab| tab.path == path && !matches!(tab.kind, FileKind::Diff { .. }))
            .map(|tab| tab.kind)
    }

    /// 덮어쓰기 묻기(`Overwrite`): 디스크가 바뀐 것을 알면서 이 파일을 저장한다. 기다리는 다른 묻기가 있으면 이어서 묻는다.
    pub(crate) fn save_overwriting(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        let prior = self.detached_file_context.take();
        self.save_file_path(path, true, cx);
        self.show_next_overwrite(window, cx);
        if self.overlay == Overlay::None {
            match prior {
                Some(key) => self.activate_detached_file(&key, cx),
                None => self.focus_selected(window, cx),
            }
        }
        cx.notify();
    }

    /// Ctrl+G: 줄 번호를 묻는 창을 연다. 지금 파일 탭이 텍스트 편집기일 때만 연다.
    pub(crate) fn begin_go_to_line(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let has_editor = self.active_file_path().is_some_and(|path| self.text_body_of(&path).is_some());
        if has_editor {
            self.settle_composition(window, cx);
            let input = cx.new(|cx| InputState::new(window, cx));
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } if this.goto_input.as_ref() == Some(input) => {
                    this.commit_go_to_line(window, cx)
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            })
            .detach();
            self.goto_input = Some(input.clone());
            let n_lines = self
                .active_file_path()
                .and_then(|path| self.text_body_of(&path).map(|body| body.editor.read(cx).value().lines().count()))
                .unwrap_or(1)
                .max(1);
            self.open_overlay(Overlay::GoToLine { n_lines }, window, cx);
            input.update(cx, |state, cx| state.focus(window, cx));
        }
        has_editor
    }

    /// 줄로 가기 창에 적힌 줄 번호(1부터). 숫자가 아니거나 0이면 None이다. 파일 줄 수보다 크면 마지막 줄이다.
    pub(crate) fn go_to_line_target(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<usize> {
        let typed = self.goto_input.as_ref().map(|input| input.read(cx).value().to_string());
        match (typed, &self.overlay) {
            (Some(typed), Overlay::GoToLine { n_lines }) => paddock_core::text_file::line_target(&typed, *n_lines),
            _ => None,
        }
    }

    /// 줄로 가기 창의 `OK`·Enter: 적힌 줄로 간다.
    pub(crate) fn commit_go_to_line(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(line) = self.go_to_line_target(cx) {
            self.goto_input = None;
            self.overlay = Overlay::None;
            self.go_to_line_now(line, window, cx);
            if !self.end_detached_file_context(cx) {
                self.focus_selected(window, cx);
            }
            cx.notify();
        }
    }

    /// 지금 파일 탭의 편집기를 `line`번째(1부터) 줄로 옮긴다.
    pub(crate) fn go_to_line_now(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.active_file_path() {
            match self.file_views.get_mut(&path) {
                Some(FileContent::Text(body)) => {
                    scroll_to_line(body, line, window, cx);
                }
                Some(FileContent::Markdown(body)) => {
                    if let Some(source) = body.source.as_ref() {
                        scroll_to_line(source, line, window, cx);
                    }
                }
                _ => {}
            }
        }
    }

    /// 상태 줄 저장 표시: 지금 파일 탭이 저장하지 않았으면 `Unsaved`, 방금 저장했으면(2초 안) `Saved`, 아니면 None.
    pub(crate) fn save_status(&self) -> Option<paddock_core::unsaved::SaveStatus> {
        self.active_file_path().and_then(|path| {
            let saved_recently =
                self.saved_flash.as_ref().is_some_and(|(flash, at)| *flash == path && at.elapsed() < SAVED_SHOWN);
            paddock_core::unsaved::save_status(self.unsaved.contains(&path), saved_recently)
        })
    }
}
