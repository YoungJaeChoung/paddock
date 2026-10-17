//! Source control 동작: 보기 바꾸기, 스테이징·커밋·되돌리기, 변경 비교 탭 열기·갱신, Git 상태가 창 사건에 맞춰 갱신되게 하는 연결.
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::{AppContext, Context, Window};
use paddock_core::file_kind::FileKind;
use paddock_core::file_pane::FileTab;
use paddock_core::file_tree::child_path;
use paddock_core::git_status::{Change, Section};
use paddock_core::work_model::is_inside;

use super::SidebarView;
use super::diff_view::{build_diff, parse_patch};
use super::model::{CommitState, Group, commit_state, paths_to_stage};
use crate::files::pane::FileContent;
use crate::git::gate::Trigger;
use crate::git::service::WriteKind;
use crate::git::state::Repo;
use crate::window_view::{Overlay, WorkWindow};
use gpui_kit::component::diff::DiffFile;

impl WorkWindow {
    /// 사이드바가 보여 줄 보기를 바꾼다. Source control 보기를 열면 저장소 상태를 한 번 새로 읽는다.
    pub(crate) fn show_sidebar_view(
        &mut self,
        view: SidebarView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scm.view != view {
            self.scm.view = view;
            // 빠른 설정은 보기 줄에서 열리므로 보기를 바꾸면 닫는다.
            self.close_quick_settings(window, cx);
            if view == SidebarView::SourceControl {
                self.ensure_commit_input(window, cx);
                self.git.note(Trigger::Entered, Instant::now());
            }
            self.focus_selected(window, cx);
            cx.notify();
        }
    }

    /// 커밋 메시지 입력칸이 없으면 만든다. 입력칸 안의 Ctrl+Enter는 커밋이다. 입력칸의 줄 높이가 기존 화면보다 커서 한 줄에서 시작해
    /// 늘어나게 하고, 보이는 상자의 최소 높이(두 줄)는 보기가 정한다.
    fn ensure_commit_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scm.commit_input.is_none() {
            let input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 6).placeholder("Message"));
            cx.subscribe_in(&input, window, |this, _input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { secondary: true, .. } => this.scm_commit(window, cx),
                InputEvent::Change => {
                    this.scm.note = None;
                    cx.notify();
                }
                _ => {}
            })
            .detach();
            self.scm.commit_input = Some(input);
        }
    }

    /// 커밋 입력칸에 든 글자. 입력칸이 없으면 빈 글자다.
    pub(crate) fn commit_message(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        self.scm.commit_input.as_ref().map(|input| input.read(cx).value().to_string()).unwrap_or_default()
    }

    /// 머리줄 새로 고침 단추: 바로 다시 읽는다.
    pub(crate) fn scm_refresh(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.note(Trigger::Written, Instant::now());
        cx.notify();
    }

    pub(crate) fn scm_toggle_group(
        &mut self,
        group: Group,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.scm.collapsed.remove(&group) {
            self.scm.collapsed.insert(group);
        }
        cx.notify();
    }

    pub(crate) fn scm_stage(
        &mut self,
        path: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.stage(path);
        cx.notify();
    }

    pub(crate) fn scm_unstage(
        &mut self,
        path: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.unstage(path);
        cx.notify();
    }

    pub(crate) fn scm_stage_all(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.stage_all();
        cx.notify();
    }

    /// 구획 머리의 `+`: 병합 구획은 충돌 행만, 변경 구획은 충돌을 건드리지 않고 스테이징한다.
    pub(crate) fn scm_stage_group(
        &mut self,
        group: Group,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = self.git.ready().and_then(|repo| paths_to_stage(&repo.status, group));
        self.git.stage_group(paths);
        cx.notify();
    }

    pub(crate) fn scm_unstage_all(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.unstage_all();
        cx.notify();
    }

    /// 입력칸의 메시지로 스테이징된 변경을 커밋한다. 메시지가 비었으면 입력을 요구하는 한 줄만 보인다.
    pub(crate) fn scm_commit(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.git.is_committing() {
            return;
        }
        let n_staged = self.git.ready().map_or(0, |repo| repo.status.n_in(Section::Staged));
        let message = self.commit_message(cx);
        match commit_state(n_staged, &message) {
            CommitState::Ready => {
                self.scm.note = None;
                self.git.commit(&message);
            }
            CommitState::NeedsMessage => self.scm.note = Some("Enter a commit message.".to_string()),
            CommitState::NothingStaged => {}
        }
        cx.notify();
    }

    /// 변경 행을 누르면: 추적 중인 변경은 비교 탭을, 추적하지 않는 새 파일은 읽기 전용 파일 탭을 연다.
    pub(crate) fn scm_open_row(
        &mut self,
        change: &Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(root) = self.git.ready().map(|repo| repo.root.clone()) {
            let path = child_path(&root, &change.path);
            match change.section {
                Section::Untracked => self.open_file(&path, window, cx),
                Section::Staged => self.open_file_as(&path, FileKind::Diff { staged: true }, window, cx),
                Section::Merge | Section::Changes => {
                    self.open_file_as(&path, FileKind::Diff { staged: false }, window, cx)
                }
            }
        }
    }

    /// 변경 되돌리기 확인 창을 연다.
    pub(crate) fn scm_ask_discard(
        &mut self,
        change: Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_overlay(Overlay::Discard(change), window, cx);
    }

    /// 확인 창에서 승인한 되돌리기를 실행한다.
    pub(crate) fn scm_confirm_discard(
        &mut self,
        change: &Change,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git.discard(change);
        self.cancel_overlay(window, cx);
    }

    /// 변경 비교 읽기를 시작한다. 읽는 동안 이미 보이던 내용이 있으면 그대로 두고, 없으면 빈 자리를 보인다.
    pub(crate) fn start_diff_load(
        &mut self,
        path: String,
        staged: bool,
    ) {
        let kind = FileKind::Diff { staged };
        let key = paddock_core::file_pane::tab_key(&path, kind);
        match self.git.change_for(&path, staged) {
            Some(change) => {
                self.file_views.entry(key.clone()).or_insert(FileContent::Loading);
                self.git.request_diff(key, change);
            }
            None => {
                self.file_views.insert(key, FileContent::Failed("This file has no changes.".to_string()));
            }
        }
    }

    fn finish_diff_load(
        &mut self,
        key: String,
        result: Result<Vec<DiffFile>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.files().open_keys().any(|open| open == key) {
            let content = match result {
                Ok(files) => FileContent::Diff(build_diff(files, cx)),
                Err(message) => FileContent::Failed(message),
            };
            self.file_views.insert(key, content);
        }
    }

    /// 저장소가 바뀌었을 때 열려 있는 비교 탭을 맞춘다: 변경이 사라진 탭은 닫고, 남은 탭은 다시 읽는다.
    /// 지금 읽은 저장소 밖 파일의 비교 탭은 건드리지 않는다.
    fn refresh_diff_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(root) = self.git.ready().map(|repo| repo.root.clone()) {
            self.refresh_diff_tabs_of(&root, window, cx);
        }
    }

    fn refresh_diff_tabs_of(
        &mut self,
        root: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.workspace.view_key();
        let mut gone: Vec<(paddock_core::work_model::GroupKey, usize)> = Vec::new();
        let mut reload: Vec<(String, bool, bool)> = Vec::new();
        for (key, pane) in self.workspace.files().iter() {
            for (index, tab) in pane.tabs.iter().enumerate().rev() {
                if let FileKind::Diff { staged } = tab.kind
                    && is_inside(root, &tab.path)
                {
                    if self.git.change_for(&tab.path, staged).is_none() {
                        gone.push((key.clone(), index));
                    } else {
                        let visible = shown.as_ref() == Some(key) && pane.selected == index;
                        reload.push((tab.path.clone(), staged, visible));
                    }
                }
            }
        }
        for (path, staged, visible) in reload {
            if visible {
                self.start_diff_load(path, staged);
            } else {
                self.file_views.remove(&paddock_core::file_pane::tab_key(&path, FileKind::Diff { staged }));
            }
        }
        for (key, index) in gone {
            if shown.as_ref() == Some(&key) {
                self.close_file_tab(index, window, cx);
            } else {
                self.workspace.close_file(&key, index);
            }
        }
        self.prune_file_views();
    }

    /// 창 사건을 Git 실행 시점 사건으로 옮긴다: 이 폴더 안 터미널의 출력, 창이 활성으로 돌아옴.
    fn sync_git_gates(
        &mut self,
        window: &Window,
    ) {
        let now = Instant::now();
        let latest: Vec<(String, u32, Option<Instant>)> = self
            .workspace
            .tabs()
            .iter()
            .map(|tab| {
                (
                    tab.daemon.clone(),
                    tab.id,
                    tab.content.session.grid.shared.last_output_at.lock().ok().and_then(|at| *at),
                )
            })
            .collect();
        let changed = self.scm.output.observe(latest.iter().map(|(daemon, id, at)| (daemon.as_str(), *id, *at)));
        let output_seen = changed.into_iter().any(|id| {
            self.workspace
                .tab(id)
                .and_then(|tab| tab.content.session.grid.shared.state.lock().ok().and_then(|state| state.clone()))
                .and_then(|state| state.cwd)
                .is_some_and(|cwd| self.git.covers(&cwd))
        });
        if output_seen {
            self.git.note(Trigger::Output, now);
        }
        let active = window.is_window_active();
        if active && !self.scm.was_active {
            self.git.note(Trigger::Entered, now);
        }
        self.scm.was_active = active;
    }

    /// 200 ms 갱신마다 부른다: 사건 전달, 결과 반영, 쓰기 결과 처리, 열린 비교 탭 맞춤. 화면에 보이는 것이 바뀌었으면 true다.
    pub(crate) fn pump_git(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_git_gates(window);
        let mut changed = self.git.tick(Instant::now());
        while let Some(done) = self.git.take_written() {
            changed = true;
            match (done.kind, done.result) {
                (WriteKind::Commit, Ok(())) => {
                    self.scm.note = None;
                    if let Some(input) = self.scm.commit_input.clone() {
                        input.update(cx, |state, cx| state.set_value("", window, cx));
                    }
                }
                (WriteKind::Commit, Err(message)) => self.scm.note = Some(message),
                (_, Ok(())) => {}
                (_, Err(message)) => self.status = Some(format!("Git: {message}")),
            }
        }
        while let Some((key, result)) = self.git.take_diff() {
            changed = true;
            // 큰 비교는 읽는 데 시간이 걸리므로 작업 스레드에서 읽고 보기만 창 스레드에서 만든다.
            cx.spawn_in(window, async move |this, cx| {
                let parsed =
                    cx.background_executor().spawn(async move { result.and_then(|patch| parse_patch(&patch)) }).await;
                let _ = this.update_in(cx, |this, _window, cx| {
                    this.finish_diff_load(key, parsed, cx);
                    cx.notify();
                });
            })
            .detach();
        }
        if self.git.generation() != self.scm.seen_generation && matches!(self.git.repo(), Repo::Ready(_)) {
            self.scm.seen_generation = self.git.generation();
            self.refresh_diff_tabs(window, cx);
            changed = true;
        }
        changed
    }
}

/// 탭 하나가 비교 탭인지.
pub fn is_diff_tab(tab: &FileTab) -> bool {
    matches!(tab.kind, FileKind::Diff { .. })
}
