//! 같은 탭에서 계정 바꾸기: `Continue with …`(같은 대화를 다른 계정으로 이어 열기)와 `Start in this terminal with …`(빈 셸 탭에서 계정 시작).
//!
//! 이어 열기는 이 순서다: 대화 파일을 찾는다(없으면 터미널은 그대로) → 대상 계정의 터미널 요청을 만든다(`--resume <대화 파일 경로>`) →
//! 데몬에 옛 CLI를 멈추게 한다(검증된 프로세스만, 안 끝나면 문장으로 끝) → 새 터미널을 열어 같은 탭 자리에 넣고 옛 터미널을 끝낸다 →
//! 새 CLI가 같은 대화를 이어 열었는지 확인한다. 데몬의 터미널 번호는 프로세스마다 새로 매겨져, 탭은 새 번호로 자리를 물려받는다.
//! 파일을 읽고 데몬에 묻는 일은 모두 앱의 작업 스레드가 하고, 창은 결과만 반영한다.
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{AsyncWindowContext, Context, WeakEntity, Window};
use paddock_core::account_session::{SessionIdentity, SwitchMode, check_continue, switch_mode};
use paddock_core::agent_account::Runtime;
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Kill, TerminalState, kind};

use super::switch_job::{ContinueJob, Outcome, config_folders, kill_unplaced, run_continue, run_start, source_of};
use crate::settings::Settings;
use crate::terminal::session::TerminalSession;
use crate::window_view::{TabContent, WorkWindow};
use crate::work::menu_model::{MenuKind, SwitchMenu};

/// 보고 있는 터미널의 상태(없으면 None).
fn state_of(
    window: &WorkWindow,
    id: u32,
) -> Option<(TerminalState, String, Option<String>)> {
    let tab = window.workspace.tab(id)?;
    let state = tab.content.session.grid.shared.state.lock().ok()?.clone()?;
    Some((state, tab.content.session.shell.clone(), tab.content.session.wsl_distribution.clone()))
}

impl WorkWindow {
    /// 계정 고르기를 누를 때 보고 있는 터미널에 맞는 메뉴를 연다: 대화가 있는 Claude면 `Continue`, 빈 셸이면 `Start`, 그 밖은 새 터미널 메뉴.
    pub(crate) fn open_account_picker(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = self.workspace.new_place();
        let selected = if self.file_active() { None } else { self.workspace.selected() };
        let seen = selected.and_then(|id| state_of(self, id).map(|state| (id, state)));
        let kind = seen.and_then(|(id, (state, shell, distribution))| {
            let source = source_of(&state, &shell, distribution.as_deref());
            let mode = switch_mode(state.program.as_deref(), state.conversation.is_some() && source.is_some());
            match mode {
                SwitchMode::New => None,
                SwitchMode::Continue => {
                    Some(MenuKind::AccountSwitch(SwitchMenu { terminal: id, mode, source, place: place.clone() }))
                }
                SwitchMode::Start => {
                    Some(MenuKind::AccountSwitch(SwitchMenu { terminal: id, mode, source: None, place: place.clone() }))
                }
            }
        });
        match kind {
            Some(kind) if self.has_registered_accounts() => self.open_menu(kind, position, window, cx),
            _ => self.open_new_terminal_menu(place, position, true, window, cx),
        }
    }

    /// 터미널 `terminal`의 대화를 계정 `id`로 같은 탭에서 이어 연다(`Continue with …`). 실패하면 터미널은 그대로이고 이유가 상태 줄에 보인다.
    pub(crate) fn switch_terminal_account(
        &mut self,
        terminal: u32,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.prepare_continue(terminal, id) {
            Err(message) => self.status = Some(message),
            Ok(job) => {
                self.switching.insert(terminal);
                let session_id = job.conversation.session_id.clone();
                // 탭 이름이 이 계정 이름이거나 에이전트가 올라오며 붙은 이름(`claude`)이면 새 계정 이름으로 바꾼다. 사용자가 지은 이름은 그대로 둔다.
                let previous_names = vec![
                    crate::agent_view::account_label(state_of(self, terminal).map(|state| state.0).as_ref()),
                    "claude".to_string(),
                ];
                cx.spawn_in(window, async move |this, cx| {
                    let outcome = cx.background_executor().spawn(async move { run_continue(job) }).await;
                    finish(this, cx, terminal, outcome, previous_names, Some(session_id)).await;
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// 이어 열기에 필요한 값을 모은다. 창이 쥔 상태만 읽고 파일은 읽지 않는다.
    fn prepare_continue(
        &self,
        terminal: u32,
        id: &str,
    ) -> Result<ContinueJob, String> {
        (!self.switching.contains(&terminal)).then_some(()).ok_or(SessionIdentity::ALREADY_SWITCHING)?;
        let (state, shell, distribution) = state_of(self, terminal)
            .ok_or("The original terminal is no longer open. Select a terminal and try again.")?;
        let conversation = state
            .conversation
            .clone()
            .filter(|conversation| conversation.provider == "claude")
            .ok_or(SessionIdentity::NO_CONVERSATION)?;
        let source = source_of(&state, &shell, distribution.as_deref()).ok_or(SessionIdentity::NO_CONVERSATION)?;
        let target = self
            .registered_accounts()
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or("The selected account is no longer available. Refresh the account list.")?;
        check_continue(&source, &target).map_err(str::to_string)?;
        let (origin, current) = config_folders(&state, &source, &conversation)?;
        let folder = if target.runtime == Runtime::Wsl {
            Some(conversation.cwd.clone())
        } else {
            state.cwd.clone().or(Some(conversation.cwd.clone()))
        };
        let daemon_exe = self.daemon_exe_ready().ok_or("The terminal daemon is not ready yet.")?;
        Ok(ContinueJob {
            daemon_exe,
            terminal,
            conversation,
            target,
            origin,
            current,
            folder,
            home: std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default(),
            permissions: Settings::load().agent_permissions(),
        })
    }

    /// 빈 셸 탭에서 계정 `id`를 시작한다(`Start in this terminal with …`). 셸이 아닌 프로그램이 돌고 있으면 거부한다.
    pub(crate) fn start_account_here(
        &mut self,
        terminal: u32,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.switching.contains(&terminal) {
            self.status = Some(SessionIdentity::ALREADY_SWITCHING.to_string());
        } else {
            let state = state_of(self, terminal).map(|state| state.0);
            let want = self.account_terminal_request_at(id, state.as_ref().and_then(|state| state.cwd.clone()));
            match (want, self.daemon_exe_ready()) {
                (Err(message), _) => self.status = Some(message),
                (Ok(_), None) => self.status = Some("The terminal daemon is not ready yet.".to_string()),
                (Ok(want), Some(daemon_exe)) => {
                    self.switching.insert(terminal);
                    let previous_names = vec![crate::agent_view::account_label(state.as_ref())];
                    cx.spawn_in(window, async move |this, cx| {
                        let outcome =
                            cx.background_executor().spawn(async move { run_start(daemon_exe, terminal, want) }).await;
                        finish(this, cx, terminal, outcome, previous_names, None).await;
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    /// 데몬이 준비되었을 때만 그 실행 파일.
    fn daemon_exe_ready(&self) -> Option<PathBuf> {
        self.daemon_exe_if_ready()
    }

    /// 탭 `old`의 터미널을 새 터미널로 바꾼다: 같은 탭 자리·칸·Work 목록 행에 넣고 옛 터미널을 끝낸다.
    /// 탭 이름이 옛 계정 이름이었으면 새 계정 이름으로 바꾼다. 새 터미널의 번호를 돌려준다.
    pub(crate) fn replace_tab_terminal(
        &mut self,
        old: u32,
        session: TerminalSession,
        title: Option<&str>,
        previous_names: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u32> {
        let view = self.view_of(&session, window, cx);
        let (daemon, new, outbox) = (session.daemon.clone(), session.id, session.outbox.clone());
        let was_named_after_account =
            previous_names.iter().any(|name| !name.is_empty() && self.workspace.name(old) == name.as_str());
        let replaced = self.workspace.replace_terminal(old, new, &daemon, TabContent { session, view });
        match &replaced {
            Some(tab) => {
                let _ = tab.content.session.outbox.send(&Frame::json(kind::KILL, &Kill { id: old }));
                self.forget_terminal_state(old);
                if was_named_after_account && let Some(title) = title {
                    self.workspace.rename(new, title);
                }
                // 교체 직후의 새 번호도 확인이 끝날 때까지 전환 중으로 둔다(그 사이 다시 고르면 거부한다).
                self.switching.insert(new);
                // 다른 창에 있던 탭이면 그 창이 새 터미널을 이어서 보인다(창을 닫고 다시 열지 않는다).
                self.retarget_detached_window(old, new, cx);
                self.focus_selected(window, cx);
                self.persist();
            }
            // 그 사이 탭이 닫혔다. 놓을 자리가 없는 새 터미널은 데몬에서 끝낸다.
            None => {
                kill_unplaced(&outbox, new);
                self.forget_terminal_state(new);
            }
        }
        replaced.map(|_| new)
    }

    /// 계정 고르기 메뉴의 모양을 한 줄로: 자동 점검이 읽는다. `account-menu kind=continue items=B,Current(A)`.
    #[cfg(feature = "automation")]
    pub(crate) fn account_menu_summary(&self) -> Option<String> {
        let menu = self.menu.as_ref()?;
        let (kind, items) = match &menu.kind {
            MenuKind::AccountSwitch(switch) => Some((
                if switch.mode == SwitchMode::Continue { "continue" } else { "start" },
                self.menu_entries(&menu.kind),
            )),
            MenuKind::NewTerminal(_) => Some(("new", self.menu_entries(&menu.kind))),
            _ => None,
        }?;
        let names: Vec<String> = items
            .iter()
            .map(|item| {
                let name = item.label.rsplit(" \u{b7} ").next().unwrap_or(&item.label).to_string();
                if item.meta == "Current" { format!("Current({name})") } else { name }
            })
            .collect();
        Some(format!("account-menu kind={kind} items={}", names.join(",").replace(' ', "_")))
    }
}

/// 끝난 이어 열기·계정 시작을 창에 반영한다: 성공하면 탭을 바꾸고, 이어 열기면 새 CLI가 같은 대화를 이었는지 확인한다.
async fn finish(
    this: WeakEntity<WorkWindow>,
    cx: &mut AsyncWindowContext,
    terminal: u32,
    outcome: Outcome,
    previous_names: Vec<String>,
    expected_session: Option<String>,
) {
    let replaced = this
        .update_in(cx, |this, window, cx| match outcome {
            Ok((session, title)) => {
                this.replace_tab_terminal(terminal, session, title.as_deref(), &previous_names, window, cx)
            }
            Err(message) => {
                this.status = Some(message);
                None
            }
        })
        .ok()
        .flatten();
    let mut unconfirmed = false;
    if let (Some(new), Some(session_id)) = (replaced, expected_session) {
        let mut confirmed = false;
        for attempt in 0..SessionIdentity::N_CONFIRM_ATTEMPTS {
            if attempt > 0 {
                cx.background_executor().timer(Duration::from_millis(SessionIdentity::CONFIRM_INTERVAL_MS)).await;
            }
            confirmed = this
                .update_in(cx, |this, _window, _cx| {
                    state_of(this, new)
                        .and_then(|state| state.0.conversation)
                        .is_some_and(|conversation| conversation.session_id == session_id)
                })
                .unwrap_or(true);
            if confirmed {
                break;
            }
        }
        unconfirmed = !confirmed;
    }
    let _ = this.update_in(cx, |this, _window, cx| {
        this.switching.remove(&terminal);
        if let Some(new) = replaced {
            this.switching.remove(&new);
        }
        if unconfirmed {
            this.status = Some(SessionIdentity::UNCONFIRMED.to_string());
        }
        cx.notify();
    });
}
