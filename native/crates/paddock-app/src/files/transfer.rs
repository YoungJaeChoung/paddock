//! 파일 목록이 밖의 파일을 받는 동작과 이름 바꾸기: 클립보드 붙여넣기(Ctrl+V·메뉴 `Paste`), 탐색기에서 끌어 놓기, `Rename...`·F2.
//!
//! 디스크에 쓰는 일은 모두 작업 스레드(`background_executor`)에서 하고(`ops`), 결과만 창으로 와 상태 줄과 목록·열린 탭에 반영한다. 실패는 그 일의 결과로 가둔다.
use std::time::Instant;

use gpui_kit::{Context, ExternalPaths, Window};
use paddock_core::file_tree::renamed;
use paddock_core::file_tree::{ListFocusEvent, ListKey, list_key};
use paddock_core::incoming::{ClipboardPart, Incoming, copied_message, incoming_of, paste_destination};
use paddock_core::paths::{file_name_of, rebased};

use super::clipboard::parts_of;
use super::open::{FileStamp, file_stamp};
use super::pane::FileContent;
use super::write_service::{WriteCommand, WriteResult};
use crate::git::gate::Trigger;
use crate::window_view::{RenameTarget, WorkWindow};

/// 클립보드에 파일도 그림도 없을 때 상태 줄에 보이는 안내.
pub const NOTHING_TO_PASTE: &str = "The clipboard has no files or images. Copy a file or take a screenshot first.";

impl WorkWindow {
    /// 목록이 키를 받고 있을 때 F2·Ctrl+V를 가로챈다. 가로챘으면 true다. 목록이 키를 받지 않거나(터미널·편집기가 받는 중) 다른 키면 목록은 키를 거두고 false다.
    pub(crate) fn list_key_pressed(
        &mut self,
        key: &str,
        modifiers: (bool, bool, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let entry = self.files.focused_entry();
        match (entry, list_key(key, modifiers.0, modifiers.1, modifiers.2)) {
            (Some((path, is_dir)), Some(ListKey::Rename)) => {
                self.begin_rename(RenameTarget::Entry { path, is_dir }, window, cx);
                true
            }
            (Some((path, is_dir)), Some(ListKey::Paste)) => {
                self.paste_into_files(Some((path, is_dir)), window, cx);
                true
            }
            _ => {
                self.files.focus_event(ListFocusEvent::OtherKey);
                false
            }
        }
    }

    /// 파일 목록의 행을 눌렀다(마우스 누름·자동 점검의 `act files`·`act openfile`이 함께 쓰는 한 길): 폴더는 접고 펼치고 파일은 파일 탭으로 연다.
    /// 그 행이 선택되고 목록이 키(F2·Ctrl+V)를 받는다. 키를 터미널·편집기로 돌려주는 동작(`open_file`·`toggle_files_dir`의 포커스 이동)보다 나중에 알려야 하므로 끝에서 한다.
    pub(crate) fn press_file_row(
        &mut self,
        path: &str,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_dir {
            self.toggle_files_dir(path, window, cx);
        } else {
            self.open_file(path, window, cx);
        }
        self.files.select_row(path);
    }

    /// Ctrl+V·메뉴 `Paste`: 클립보드의 파일 목록(없으면 그림)을 `row`의 폴더에 받는다. 파일도 그림도 없으면 상태 줄에 안내한다.
    pub(crate) fn paste_into_files(
        &mut self,
        row: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parts: Vec<ClipboardPart> = cx.read_from_clipboard().map(|item| parts_of(&item)).unwrap_or_default();
        self.receive_from(incoming_of(&parts), row, window, cx);
    }

    /// 탐색기에서 끌어 온 파일을 `row`의 폴더(행이 없으면 목록 폴더)에 받는다.
    pub(crate) fn drop_into_files(
        &mut self,
        dropped: &ExternalPaths,
        row: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths: Vec<String> = dropped.paths().iter().map(|path| path.to_string_lossy().to_string()).collect();
        self.drop_paths_into_files(paths, row, window, cx);
    }

    /// `drop_into_files`와 같되 경로 글자 목록을 받는다(자동 점검이 끌어 놓기를 흉내 낼 때 쓴다).
    pub(crate) fn drop_paths_into_files(
        &mut self,
        paths: Vec<String>,
        row: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let incoming = Some(Incoming::Paths(paths))
            .filter(|incoming| matches!(incoming, Incoming::Paths(paths) if !paths.is_empty()));
        self.receive_from(incoming, row, window, cx);
    }

    /// `incoming`을 `row`가 정하는 폴더에 받는다. 받을 것이 없으면(None) 안내하고, 받을 폴더가 없으면 아무것도 하지 않는다.
    fn receive_from(
        &mut self,
        incoming: Option<Incoming>,
        row: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match incoming {
            None => {
                self.status = Some(NOTHING_TO_PASTE.to_string());
                cx.notify();
            }
            Some(incoming) => {
                let destination = paste_destination(
                    row.as_ref().map(|(path, is_dir)| (path.as_str(), *is_dir)),
                    self.files.creation_folder().as_deref(),
                );
                if let Some(destination) = destination {
                    self.receive_into(incoming, destination, window, cx);
                }
            }
        }
    }

    /// 받는 일을 쓰기 길(`WriteService`)에 맡긴다. 끝나면 `pump_writes`가 그 폴더를 펼쳐 새 항목이 보이게 하고 상태 줄에 결과를 알린다.
    fn receive_into(
        &mut self,
        incoming: Incoming,
        destination: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = None;
        self.writes.submit(match incoming {
            Incoming::Paths(sources) => WriteCommand::Copy { sources, destination },
            Incoming::Image { kind, bytes } => WriteCommand::Image { folder: destination, kind, bytes },
        });
        cx.notify();
    }

    /// 이름 바꾸기 창의 확인: 같은 폴더 안에서 이름을 바꾸고, 성공하면 열린 탭·펼침·선택·읽어 둔 내용을 새 경로로 옮긴다.
    /// 이름이 그대로이면 아무것도 하지 않는다. 다른 항목이 이미 그 이름이면 덮어쓰지 않고 상태 줄에 알린다. 안에 저장하지 않은 파일이 있으면 바꾸지 않고 먼저 저장하라고 알린다.
    pub(crate) fn rename_entry_to(
        &mut self,
        path: String,
        new_name: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if new_name != file_name_of(&path) {
            if self.unsaved.count_inside(&path) > 0 {
                // 열린 탭의 읽어 둔 내용은 이름이 바뀌면 버리고 새 경로에서 다시 읽으므로, 저장하지 않은 편집이 사라진다. 먼저 저장하게 한다.
                self.status = Some(format!(
                    "Save the unsaved changes in \u{201c}{}\u{201d} before renaming it.",
                    file_name_of(&path)
                ));
            } else {
                self.status = None;
                // 바뀐 뒤 열린 파일들의 쓴 시각은 쓰기 스레드가 재서 결과에 싣는다(창 스레드가 느린 공유 폴더를 재지 않게).
                let to = renamed(&path, &new_name);
                let watch: Vec<String> = self
                    .workspace
                    .open_file_keys()
                    .filter(|key| !key.contains('\u{1f}'))
                    .filter_map(|key| rebased(&path, &to, &key))
                    .collect();
                self.renaming.push(path.clone());
                self.writes.submit(WriteCommand::Rename { from: path, new_name, watch });
            }
            cx.notify();
        }
    }

    /// 쓰기 길에서 도착한 결과를 모두 반영한다(갱신 루프가 부른다). 화면에 보이는 것이 바뀌었으면 true다.
    pub(crate) fn pump_writes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        while let Some(result) = self.writes.try_recv() {
            changed = true;
            match result {
                WriteResult::Copied { destination, outcome } => {
                    let name = file_name_of(&destination);
                    self.status = Some(copied_message(
                        outcome.copied.len(),
                        outcome.n_refused,
                        outcome.n_unnamed,
                        outcome.failure.as_deref(),
                        &name,
                    ));
                    if !outcome.copied.is_empty() {
                        self.files.show_added(&destination, None);
                        self.git.note(Trigger::Change, Instant::now());
                    }
                }
                WriteResult::Image { folder, result } => match result {
                    Ok(path) => {
                        self.status = Some(format!("Saved {} into {}", file_name_of(&path), file_name_of(&folder)));
                        self.files.show_added(&folder, None);
                        self.git.note(Trigger::Change, Instant::now());
                    }
                    Err(message) => self.status = Some(message),
                },
                WriteResult::PastedImage { terminal, result } => match result {
                    Ok(path) => {
                        let view = self.workspace.tab(terminal).map(|tab| tab.content.view.clone());
                        if let Some(view) = view {
                            view.update(cx, |view, cx| view.paste_paths(&[path], cx));
                        }
                    }
                    Err(message) => self.status = Some(message),
                },
                WriteResult::Renamed { from, result, stamps } => {
                    self.renaming.retain(|renaming| *renaming != from);
                    match result {
                        Ok(to) => self.finish_rename(&from, &to, &stamps, window, cx),
                        Err(message) => self.status = Some(message),
                    }
                }
                WriteResult::Saved { path, text, outcome } => self.finish_save(&path, &text, outcome, window, cx),
                WriteResult::Replaced { outcome } => self.finish_replace(outcome, window, cx),
            }
        }
        // 다른 묻기 창이 닫힌 뒤(어떤 길로 닫혔든) 기다리던 덮어쓰기 묻기를 이어서 띄운다.
        self.show_next_overwrite(window, cx);
        changed
    }

    /// 이름이 바뀐 뒤 앱의 기억을 새 경로로 옮긴다. 열린 탭은 계속 활성이고, 옮겨진 탭의 읽어 둔 내용은 버려 새 경로에서 다시 읽는다(저장하지 않은 편집이 있는 파일만 편집기를 그대로 옮긴다).
    fn finish_rename(
        &mut self,
        from: &str,
        to: &str,
        stamps: &[(String, Option<FileStamp>)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let moved_tabs = self.workspace.rename_file_paths(from, to);
        self.retarget_detached_file_windows(from, to, cx);
        self.files.renamed(from, to);
        self.git.note(Trigger::Change, Instant::now());
        let is_moved = |path: &str| rebased(from, to, path).is_some();
        let tab_path = |key: &str| key.rsplit('\u{1f}').next().unwrap_or(key).to_string();
        // 이름을 바꾸는 사이에 고친 파일은 편집기를 버리지 않고 새 경로 밑으로 옮긴다(저장하지 않은 표시도 함께).
        let edited: Vec<String> = self.unsaved.paths().filter(|path| is_moved(path)).cloned().collect();
        let mut kept: Vec<(String, FileContent)> = Vec::new();
        for old in edited {
            if let (Some(new), Some(content)) = (rebased(from, to, &old), self.file_views.remove(&old)) {
                kept.push((new, content));
            }
            self.unsaved.clear(&old);
            self.disk_changed.remove(&old);
        }
        self.file_views.retain(|key, _| !is_moved(&tab_path(key)));
        self.file_stamps.retain(|key, _| !is_moved(&tab_path(key)));
        for (new, content) in kept {
            // 쓰기 스레드가 잰 쓴 시각을 쓴다(없으면 — 요청 뒤에 열린 파일 — 지금 잰다).
            let stamp =
                stamps.iter().find(|(path, _)| *path == new).map_or_else(|| file_stamp(&new), |(_, stamp)| *stamp);
            self.file_stamps.insert(new.clone(), stamp);
            self.unsaved.mark(&new);
            self.file_views.insert(new, content);
        }
        let forced: Vec<String> = self.forced_text.iter().filter(|path| is_moved(path)).cloned().collect();
        for old in forced {
            self.forced_text.remove(&old);
            self.forced_text.extend(rebased(from, to, &old));
        }
        self.markdown_views.renamed(from, to);
        let lines: Vec<(String, usize)> = self
            .pending_lines
            .iter()
            .filter(|(path, _)| is_moved(path))
            .map(|(path, line)| (path.clone(), *line))
            .collect();
        for (old, line) in lines {
            self.pending_lines.remove(&old);
            if let Some(new) = rebased(from, to, &old) {
                self.pending_lines.insert(new, line);
            }
        }
        if moved_tabs {
            self.persist();
            self.load_visible_file(window, cx);
        }
    }
}
