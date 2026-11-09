//! 폴더 검색 동작: 보기 열기·닫기, 질의 실행, 결과 행 누름, 옵션 단추, 검색 범위 따라가기.
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext, Context, Focusable, Window};
use paddock_core::file_tree::hidden_names;
use paddock_core::paths::file_name_of;
use paddock_core::search_model::replace_report;
use paddock_core::search_scope::search_folder;

use super::engine::SearchRequest;
use crate::files::ops::{ReplaceJob, ReplaceOutcome};
use crate::files::write_service::WriteCommand;
use crate::git::gate::Trigger;
use crate::scm::SidebarView;
use crate::settings::Settings;
use crate::terminal_link::{home_folder, terminal_cwd};
use crate::window_view::{Overlay, WorkWindow};

/// 검색이 도는 동안 결과를 창으로 옮기는 간격.
const PUMP_INTERVAL: Duration = Duration::from_millis(50);

impl WorkWindow {
    /// 검색할 폴더: 파일 구획이 보여 주는 폴더, 없으면 선택한 터미널의 현재 폴더. 둘 다 없으면 None이다.
    pub(crate) fn search_scope_now(&self) -> Option<String> {
        let cwd = self
            .workspace
            .selected()
            .and_then(|id| self.workspace.tab(id))
            .and_then(|tab| terminal_cwd(&tab.content.session.grid));
        search_folder(self.files.folder(), cwd.as_deref(), home_folder().as_deref())
    }

    /// Ctrl+Shift+F와 머리줄 검색 단추: 검색 보기가 닫혀 있으면 열고 열려 있으면 닫는다.
    pub(crate) fn toggle_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.open {
            self.close_search(window, cx);
        } else {
            self.open_search(window, cx);
        }
    }

    /// 검색 보기를 연다. 사이드바가 접혀 있으면 펴고, Source control 보기였으면 Work 보기로 돌리고, 접힌 파일 구획은 펴서 입력칸을 보인다.
    /// 검색할 폴더가 없으면(홈 폴더·파일 시스템 루트의 터미널) 입력칸 자리에 안내 문장만 보인다.
    pub(crate) fn open_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.sidebar_collapsed() {
            self.toggle_sidebar(window, cx);
        }
        self.show_sidebar_view(SidebarView::Work, window, cx);
        self.files.files_expanded = true;
        self.search.open = true;
        let scope = self.search_scope_now();
        let has_scope = scope.is_some();
        self.search.set_scope(scope);
        if has_scope {
            self.ensure_search_input(window, cx);
            if let Some(input) = self.search.input.clone() {
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        cx.notify();
    }

    /// 검색 보기를 닫는다. 도는 검색을 멈추고 결과를 비워 메모리에 남기지 않으며, 키 입력은 선택한 탭으로 돌아간다.
    pub(crate) fn close_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.open {
            self.search.open = false;
            self.search.clear_results();
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 질의 입력칸이 지금 키 입력을 받고 있는지.
    pub(crate) fn search_input_focused(
        &self,
        window: &Window,
        cx: &gpui_kit::App,
    ) -> bool {
        self.search.input.as_ref().is_some_and(|input| input.focus_handle(cx).is_focused(window))
    }

    /// 질의 입력칸이 없으면 만든다. Enter가 검색 실행이다.
    pub(crate) fn ensure_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.input.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) && this.search.input.as_ref() == Some(input) {
                    this.run_search(window, cx);
                }
            })
            .detach();
            self.search.input = Some(input);
        }
    }

    /// 입력칸의 질의로 검색을 시작한다. 질의가 비었으면 도는 검색을 멈추고 결과만 비운다.
    pub(crate) fn run_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = self.search.input.as_ref().map(|input| input.read(cx).value().to_string()).unwrap_or_default();
        let request = self.search_scope_now().filter(|_| !query.is_empty()).map(|folder| SearchRequest {
            folder,
            query,
            options: self.search.options,
            hidden: hidden_names(Settings::load().values()),
        });
        let started = request.is_some();
        self.search.start(request);
        if started {
            let generation = self.search.n_started();
            cx.spawn_in(window, async move |this, cx| {
                // 도는 동안 결과를 창으로 옮긴다. 새 검색이 시작되거나 검색이 끝나면 이 반복은 끝난다.
                let mut running = true;
                while running {
                    cx.background_executor().timer(PUMP_INTERVAL).await;
                    running = this
                        .update_in(cx, |this, _window, cx| {
                            if this.search.pump() {
                                cx.notify();
                            }
                            this.search.n_started() == generation && this.search.is_running()
                        })
                        .unwrap_or(false);
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// 옵션 단추(`case`·`word`·`regex`)를 누른다. 질의가 있으면 새 옵션으로 다시 검색한다.
    pub(crate) fn toggle_search_option(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search.toggle_option(name) {
            let has_query = self.search.input.as_ref().is_some_and(|input| !input.read(cx).value().is_empty());
            if has_query {
                self.run_search(window, cx);
            }
            cx.notify();
        }
    }

    /// 결과의 파일 행을 눌렀다: 그 파일의 일치 줄을 접거나 펼친다.
    pub(crate) fn toggle_search_file(
        &mut self,
        file: usize,
        cx: &mut Context<Self>,
    ) {
        self.search.toggle_file(file);
        cx.notify();
    }

    /// 결과의 일치 줄 행을 눌렀다: 그 파일을 파일 탭으로 열고 그 줄로 간다.
    pub(crate) fn open_search_match(
        &mut self,
        file: usize,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .search
            .results()
            .files()
            .get(file)
            .and_then(|result| result.matches.get(index).map(|found| (result.path.clone(), found.line)));
        if let Some((path, line)) = target {
            self.open_file_at(&path, usize::try_from(line).unwrap_or(1), window, cx);
        }
    }

    /// 검색 범위를 지금 상태에 맞춘다. 범위가 바뀌면(없어지는 경우 포함) 이전 폴더의 결과를 비우고, 범위가 새로 생기면 입력칸을 만든다.
    /// 화면에 보이는 것이 바뀌었으면 true다.
    pub(crate) fn sync_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let scope = self.search_scope_now();
        let has_scope = scope.is_some();
        let changed = self.search.open && self.search.set_scope(scope);
        if changed && has_scope {
            self.ensure_search_input(window, cx);
        }
        changed
    }

    /// `⇄`: 바꾸기 줄을 열거나 닫는다. 열면 바꿀 글 입력칸에 포커스를 준다.
    pub(crate) fn toggle_replace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search.replace_open = !self.search.replace_open;
        if self.search.replace_open {
            if self.search.replace_input.is_none() {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace"));
                self.search.replace_input = Some(input);
            }
            if let Some(input) = self.search.replace_input.clone() {
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        cx.notify();
    }

    /// 바꿀 글(입력칸의 글 그대로 — 앞뒤 빈칸도 글자다).
    pub(crate) fn replacement_text(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        self.search.replace_input.as_ref().map(|input| input.read(cx).value().to_string()).unwrap_or_default()
    }

    /// `Replace All`: 결과 전체에 바꾸기를 할지 확인 창으로 묻는다. 결과가 없으면 아무것도 하지 않는다.
    pub(crate) fn request_replace_all(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let results = self.search.results();
        let (n_matches, n_files) = (results.n_matches(), results.files().len());
        if n_matches > 0 {
            self.open_overlay(Overlay::ReplaceAll { n_matches, n_files }, window, cx);
        }
    }

    /// 확인 창의 승인: 결과의 모든 파일에 바꾸기를 한다.
    pub(crate) fn replace_all(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = Overlay::None;
        let paths: Vec<String> = self.search.results().files().iter().map(|file| file.path.clone()).collect();
        self.replace_files(paths, window, cx);
    }

    /// 파일 행의 `Replace`: 그 파일 하나에 바꾸기를 한다.
    pub(crate) fn replace_file(
        &mut self,
        file: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.search.results().files().get(file).map(|result| result.path.clone());
        if let Some(path) = path {
            self.replace_files(vec![path], window, cx);
        }
    }

    /// `paths`에 바꾸기를 보낸다. 저장하지 않은 편집이 있는 열린 탭의 파일은 건너뛰고 상태 줄에 알린다. 디스크에 쓰는 일은 쓰기 길의 명령 하나다.
    fn replace_files(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (unsaved, files): (Vec<String>, Vec<String>) =
            paths.into_iter().partition(|path| self.unsaved.contains(&paddock_core::file_pane::file_key(path)));
        self.replace_skipped = unsaved.iter().map(|path| file_name_of(path)).collect();
        if files.is_empty() {
            self.status = Some(replace_report(0, 0, &self.replace_skipped, 0));
        } else {
            self.status = None;
            self.writes.submit(WriteCommand::Replace(ReplaceJob {
                files,
                query: self.search.query().to_string(),
                options: self.search.options,
                replacement: self.replacement_text(cx),
            }));
        }
        self.focus_selected(window, cx);
        cx.notify();
    }

    /// 바꾸기 결과: 상태 줄에 알리고, 바뀐 파일의 열린 탭을 다시 읽게 하며, 같은 질의로 검색을 다시 돌려 결과를 갱신한다.
    pub(crate) fn finish_replace(
        &mut self,
        outcome: ReplaceOutcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(match outcome.failure.as_ref() {
            Some(message) => message.clone(),
            None => replace_report(outcome.replaced.len(), outcome.n_matches, &self.replace_skipped, outcome.n_failed),
        });
        self.replace_skipped.clear();
        for path in &outcome.replaced {
            let key = paddock_core::file_pane::file_key(path);
            let kind = self.open_file_kind(&key);
            if let Some(kind) = kind {
                self.note_disk_change(key, kind, u64::MAX, window, cx);
            }
        }
        if !outcome.replaced.is_empty() {
            self.git.note(Trigger::Change, Instant::now());
            self.run_search(window, cx);
        }
        cx.notify();
    }
}
