//! 사용량 데이터: 순수 규칙(`UsageBook`)을 창이 쓰게 하는 얇은 층. 읽기는 작업 스레드가 하고 결과만 창으로 와 반영된다.
//!
//! 창 스레드는 파일도 프로세스도 건드리지 않는다. 범위마다 읽기를 스레드 하나로 시작하고, 결과를 채널로 받아
//! `UsageBook`이 표(`Ticket`)로 낡은 응답을 가려낸다. 갱신은 `AGENT_POLL_INTERVAL` 타이머가 부르는 `tick`이 120초마다(창이 가려져 있으면
//! 느리게) 시작하고, 선택이 바뀌면 바로 시작한다.
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use paddock_core::agent_account::{AccountProfile, Linked, Runtime, Scope};
use paddock_core::settings_text::{SettingValues, key};
use paddock_core::usage_scopes::{
    Change, ReadStart, RefreshPlan, Snapshot, Ticket, UsageBook, UsageScopes, UsageSelection, observe_key,
    observed_scope,
};

use super::files::{ReadContext, read_scope};
use super::{codex::CodexReader, wsl};
use crate::accounts::store::read_profiles;
use crate::app_log;

/// 가려진(포커스를 잃은) 창이 값을 다시 읽는 느린 주기.
const HIDDEN_POLL: Duration = Duration::from_secs(600);

/// 상태 줄에 보일 항목의 켬/끔(빠른 설정 `Status bar`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusItems {
    pub claude: bool,
    pub codex: bool,
    pub memory: bool,
}

impl StatusItems {
    pub fn from_settings(values: &SettingValues) -> Self {
        Self {
            claude: values.status_item(key::STATUS_BAR_CLAUDE),
            codex: values.status_item(key::STATUS_BAR_CODEX),
            memory: values.status_item(key::STATUS_BAR_MEMORY),
        }
    }

    /// 도구의 사용량 묶음을 보이는지.
    pub fn provider(
        &self,
        provider: &str,
    ) -> bool {
        match provider {
            "claude" => self.claude,
            "codex" => self.codex,
            _ => false,
        }
    }
}

/// 작업 스레드가 창에 보내는 결과.
enum Finished {
    /// 범위 하나의 사용량 읽기.
    Read { scope: Scope, ticket: Ticket, snapshot: Box<Snapshot> },
    /// 계정 목록 읽기(읽지 못했으면 None).
    Profiles(Option<Vec<AccountProfile>>),
    /// 터미널의 기본 CLI 로그인 대조: 어느 관찰(`key`)에 대한 답인지와 연결된 계정.
    Link { key: String, linked: Option<Linked> },
}

/// 로그인을 대조할 터미널의 기본 CLI.
struct LinkTarget {
    key: String,
    scope: Scope,
    runtime: Runtime,
    wsl_distribution: Option<String>,
}

/// 창이 쥐는 사용량 상태.
pub struct UsageData {
    pub book: UsageBook,
    pub items: StatusItems,
    context: Arc<ReadContext>,
    results: Receiver<Finished>,
    sender: Sender<Finished>,
    /// 마지막으로 반영한 터미널·도구·범위의 키.
    observed: Option<String>,
    last_refresh: Option<Instant>,
    was_active: bool,
    /// 계정 목록을 읽는 중이면 그 뒤 모든 계정도 읽을지.
    profiles_pending: Option<bool>,
    /// 기본 CLI가 도는 터미널(로그인을 대조할 대상). 갱신 때마다 같은 대조를 다시 한다.
    link_target: Option<LinkTarget>,
    /// 가장 최근 갱신이 건너뛴 등록 계정(기본 응답에서 연결이 풀리면 이어서 읽는다).
    plan: Option<RefreshPlan>,
    /// 새로 바뀐 저장 값(창이 state.json에 쓰고 비운다).
    pub unsaved_selection: Option<UsageSelection>,
}

impl UsageData {
    /// 저장된 선택을 되살려 만든다. 설정은 호출자가 읽어 준다.
    pub fn new(
        saved: &UsageSelection,
        items: StatusItems,
    ) -> Self {
        let (sender, results) = mpsc::channel();
        let context = ReadContext { wsl: Box::new(wsl::mapping_of), codex: Arc::new(CodexReader::default()) };
        let mut book = UsageBook::default();
        let restored = book.restore_selection(saved);
        Self {
            book,
            items,
            context: Arc::new(context),
            results,
            sender,
            observed: None,
            last_refresh: None,
            was_active: false,
            plan: None,
            link_target: None,
            profiles_pending: None,
            unsaved_selection: restored.save,
        }
    }

    /// 지금 보는 터미널의 범위로 선택을 바꾼다. 같은 터미널에서 도구·계정·연결이 바뀌었을 때만 다시 고르고 읽는다.
    /// 화면을 다시 그려야 하면 true다.
    ///
    /// 기본 CLI(등록 계정도 미등록 폴더도 아님)가 도는 터미널이면 그 실행 환경(`environment`)의 로그인을 등록 계정과 대조해
    /// 같은 로그인의 계정이 정확히 하나일 때 그 계정을 연결한다(`Scope.linked`). 연결은 그 터미널에서 CLI가 도는 동안만 믿는다.
    pub fn observe(
        &mut self,
        terminal: Option<u32>,
        account: Option<&Scope>,
        program: Option<&str>,
        environment: (Runtime, Option<&str>),
    ) -> bool {
        let scope = observed_scope(account, program);
        let key = observe_key(terminal, scope.as_ref(), program);
        let mut redraw = false;
        if self.observed.as_deref() != Some(key.as_str()) {
            self.observed = Some(key.clone());
            self.link_target = scope.clone().filter(|scope| scope.profile.is_none() && !scope.custom).map(|scope| {
                LinkTarget { key, scope, runtime: environment.0, wsl_distribution: environment.1.map(str::to_string) }
            });
            let change = self.book.select(scope);
            redraw = self.take(change);
            self.refresh(false);
        }
        redraw
    }

    /// 지금 보는 터미널의 기본 CLI 로그인을 대조하는 작업 스레드를 시작한다.
    fn start_link(&self) {
        if let Some(target) = &self.link_target {
            let (context, sender) = (self.context.clone(), self.sender.clone());
            let (key, provider, runtime, distribution) =
                (target.key.clone(), target.scope.provider.clone(), target.runtime, target.wsl_distribution.clone());
            let _ = std::thread::Builder::new().name("usage-link".to_string()).spawn(move || {
                let linked = super::login::link_of_terminal(&provider, runtime, distribution.as_deref(), &context);
                let _ = sender.send(Finished::Link { key, linked });
            });
        }
    }

    /// 범위를 직접 고른다(목록에서 계정 터미널을 연 뒤). 터미널 관찰이 같은 범위를 다시 고르기 전에도 상태 줄이 그 계정이 된다.
    pub fn choose(
        &mut self,
        scope: Scope,
    ) {
        let change = self.book.select(Some(scope));
        self.take(change);
        self.refresh(false);
    }

    /// 다시 시도: 진행 중인 응답을 버리고 모든 계정을 새로 읽는다.
    pub fn retry(&mut self) {
        self.book.preferences_changed();
        self.refresh(true);
    }

    /// 선택 변경의 저장 값을 쥐고, 다시 그려야 하는지 돌려준다.
    fn take(
        &mut self,
        change: Change,
    ) -> bool {
        if change.save.is_some() {
            self.unsaved_selection = change.save;
        }
        change.redraw
    }

    /// 상태 줄 항목 설정이 바뀌었다(켜진 도구만 읽는다).
    pub fn set_items(
        &mut self,
        items: StatusItems,
    ) {
        if items != self.items {
            self.items = items;
            self.book.preferences_changed();
            self.refresh(false);
        }
    }

    /// 갱신을 시작한다: 계정 목록을 작업 스레드가 읽고(목록이 열렸거나 아직 한 번도 못 읽었을 때), 읽은 뒤 범위마다 읽기를 시작한다.
    /// `all`이면(목록이 열림) 모든 계정도 읽는다. 목록을 읽는 중에 다시 불리면 한 번만 읽고 `all`은 합친다.
    pub fn refresh(
        &mut self,
        all: bool,
    ) {
        self.last_refresh = Some(Instant::now());
        if all || !self.book.profiles_known() {
            let already = self.profiles_pending.is_some();
            let wanted_all = self.profiles_pending.unwrap_or(false) || all;
            self.profiles_pending = Some(wanted_all);
            if !already {
                let sender = self.sender.clone();
                let spawned = std::thread::Builder::new().name("usage-profiles".to_string()).spawn(move || {
                    let listed = read_profiles().ok();
                    if listed.is_none() {
                        app_log::event("usage: the account list could not be read");
                    }
                    let _ = sender.send(Finished::Profiles(listed));
                });
                if spawned.is_err() {
                    self.profiles_pending = None;
                }
            }
        } else {
            self.start_reads(all);
        }
    }

    /// 갱신이 읽을 범위마다 읽기를 시작하고 터미널의 기본 CLI 로그인 대조도 시작한다.
    fn start_reads(
        &mut self,
        all: bool,
    ) {
        let items = self.items;
        let plan = self.book.refresh_plan(all, &|provider| items.provider(provider));
        for scope in plan.reads.clone() {
            self.start_read(&scope);
        }
        self.plan = Some(plan);
        self.start_link();
    }

    fn start_read(
        &mut self,
        scope: &Scope,
    ) {
        if let ReadStart::Fetch(ticket) = self.book.begin_read(scope) {
            let (context, sender, scope) = (self.context.clone(), self.sender.clone(), scope.clone());
            let spawned = std::thread::Builder::new().name("usage-read".to_string()).spawn(move || {
                let now = chrono::Utc::now().timestamp();
                let snapshot = read_scope(&scope, &context, cfg!(windows), now);
                let _ = sender.send(Finished::Read { scope, ticket, snapshot: Box::new(snapshot) });
            });
            if spawned.is_err() {
                app_log::event("usage: could not start a read thread");
            }
        }
    }

    /// 끝난 읽기를 반영하고, 갱신 주기가 되었으면 새 갱신을 시작한다. 화면을 다시 그려야 하면 true다.
    ///
    /// 주기는 창이 포커스를 가진 동안 120초, 잃은 동안은 느리게 쓴다. 창이 다시 포커스를 얻으면 바로 한 번 읽는다.
    pub fn tick(
        &mut self,
        window_active: bool,
        popup_open: bool,
    ) -> bool {
        let mut redraw = false;
        while let Ok(finished) = self.results.try_recv() {
            match finished {
                Finished::Read { scope, ticket, snapshot } => {
                    let applied = self.book.finish_read(&scope, ticket, *snapshot);
                    redraw |= applied;
                    if applied
                        && let Some(plan) = self.plan.clone()
                        && self.book.skipped_needs_read(&plan)
                    {
                        if let Some(skipped) = plan.skipped {
                            self.start_read(&skipped);
                        }
                        self.plan = None;
                    }
                }
                Finished::Profiles(listed) => {
                    let change = self.book.apply_profiles(listed);
                    self.take(change);
                    redraw = true;
                    if let Some(all) = self.profiles_pending.take() {
                        self.start_reads(all);
                    }
                }
                Finished::Link { key, linked } => {
                    // 그 사이 다른 터미널·도구로 옮겼으면 이 답은 낡았다.
                    if let Some(target) = self.link_target.as_ref().filter(|target| target.key == key) {
                        let current = self.book.selected(&target.scope.provider).and_then(|scope| scope.linked.clone());
                        if current != linked {
                            let scope = Scope { linked, ..target.scope.clone() };
                            let change = self.book.select(Some(scope));
                            self.take(change);
                            redraw = true;
                            // 연결이 생기거나 풀렸으니 상태 줄이 가리키는 범위가 달라진다. 그 범위를 바로 읽는다.
                            self.refresh(false);
                        }
                    }
                }
            }
        }
        let interval =
            if window_active { Duration::from_secs(UsageScopes::POLL_INTERVAL_SECONDS) } else { HIDDEN_POLL };
        let due = self.last_refresh.is_none_or(|at| at.elapsed() >= interval);
        let activated = window_active && !self.was_active;
        self.was_active = window_active;
        if (due || activated) && !self.is_reading() {
            self.refresh(popup_open);
        }
        redraw
    }

    /// 설정이 바뀌어 값을 다시 읽는다: 진행 중인 응답을 버리고 새로 시작한다.
    pub fn reread(&mut self) {
        self.book.preferences_changed();
        self.refresh(false);
    }

    /// 읽기 요청이 아직 돌고 있는지(자동 점검이 끝나기를 기다릴 때 쓴다).
    pub fn is_reading(&self) -> bool {
        self.book.is_reading() || self.profiles_pending.is_some()
    }

    /// 마지막으로 저장한 선택(state.json에 쓸 값).
    pub fn selection_to_store(&self) -> UsageSelection {
        self.book.saved_selection()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::TempHome;

    fn items() -> StatusItems {
        StatusItems { claude: true, codex: true, memory: true }
    }

    fn write(
        path: &Path,
        text: &str,
    ) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// 진행 중인 읽기가 모두 끝날 때까지 결과를 반영한다.
    fn wait_for_reads(data: &mut UsageData) {
        let end = Instant::now() + Duration::from_secs(10);
        while data.is_reading() && Instant::now() < end {
            data.tick(true, false);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!data.is_reading(), "the reads finished in time");
    }

    #[test]
    fn test_c_d74_observing_a_claude_terminal_reads_the_default_record_off_the_window_thread() {
        let home = TempHome::new();
        let config = paddock_protocol::shared_config::config_dir();
        let now = chrono::Utc::now().timestamp();
        write(
            &config.join("usage/claude.json"),
            &format!(
                r#"{{"rate_limits":{{"five_hour":{{"used_percentage":23,"resets_at":9999999999}},"seven_day":{{"used_percentage":61,"resets_at":9999999999}}}},"updated_at":{now}}}"#
            ),
        );
        write(&config.join("usage/state.json"), r#"{"claude":"on"}"#);
        let mut data = UsageData::new(&UsageSelection::default(), items());
        assert!(
            data.observe(Some(1), None, Some("claude"), (Runtime::Native, None)),
            "choosing the tool redraws the bar"
        );
        assert!(data.is_reading(), "the read runs in a worker thread");
        wait_for_reads(&mut data);
        let groups = paddock_core::usage_scopes::bar_groups(&data.book, &|provider| items().provider(provider), now);
        assert_eq!(paddock_core::usage_scopes::group_summary(&groups[0]), "claude=Default:5h=23,week=61");
        let _ = &home;
    }

    #[test]
    fn test_c_d74_a_shell_keeps_the_last_selection_and_does_not_start_a_read_for_it_twice() {
        let _home = TempHome::new();
        let mut data = UsageData::new(&UsageSelection::default(), items());
        data.observe(Some(1), None, Some("claude"), (Runtime::Native, None));
        wait_for_reads(&mut data);
        let revision = data.book.n_revision();
        assert!(
            !data.observe(Some(1), None, Some("claude"), (Runtime::Native, None)),
            "the same terminal and tool is not selected again"
        );
        assert_eq!(data.book.n_revision(), revision);
        data.observe(Some(1), None, Some("bash"), (Runtime::Native, None));
        assert!(data.book.selected("claude").is_some(), "a shell terminal leaves the bar as it was");
    }

    #[test]
    fn test_c_d74_a_tool_turned_off_in_quick_settings_is_not_read() {
        let _home = TempHome::new();
        let mut data = UsageData::new(&UsageSelection::default(), StatusItems { claude: false, ..items() });
        data.observe(Some(1), None, Some("claude"), (Runtime::Native, None));
        wait_for_reads(&mut data);
        assert_eq!(
            data.book.snapshot(&Scope::provider("claude")),
            paddock_core::usage_scopes::Snapshot::Loading,
            "Claude is off, and a tool that was not chosen is not read"
        );
        data.set_items(items());
        assert!(data.is_reading(), "turning it back on reads right away");
        wait_for_reads(&mut data);
    }

    #[test]
    fn test_c_d74_a_removed_saved_account_stays_unavailable_and_never_borrows_the_default_value() {
        let _home = TempHome::new();
        let config = paddock_protocol::shared_config::config_dir();
        write(
            &config.join("usage/claude.json"),
            r#"{"rate_limits":{"five_hour":{"used_percentage":88,"resets_at":9999999999}},"updated_at":1000}"#,
        );
        let gone = AccountProfile {
            id: "gone".to_string(),
            provider: "claude".to_string(),
            label: "Gone".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let saved = UsageSelection { claude: Some(Scope::of_profile(&gone)), codex: None };
        let mut data = UsageData::new(&saved, items());
        data.refresh(true);
        wait_for_reads(&mut data);
        assert_eq!(
            data.book.selected("claude"),
            None,
            "a restored account that no longer exists is dropped, not replaced by the default CLI"
        );
        assert_eq!(data.selection_to_store().claude, None);
    }

    #[test]
    fn test_c_d74_a_saved_account_is_restored_and_named_from_the_account_list() {
        let _home = TempHome::new();
        let config = paddock_protocol::shared_config::config_dir();
        write(
            &config.join("account-profiles.json"),
            r#"{"version":1,"profiles":[{"id":"11111111-1111-4111-8111-111111111111","provider":"claude","label":"Renamed","runtime":"native"}]}"#,
        );
        let now = chrono::Utc::now().timestamp();
        write(
            &config.join("agent-profiles/11111111-1111-4111-8111-111111111111/claude/paddock-usage/claude.json"),
            &format!(
                r#"{{"rate_limits":{{"five_hour":{{"used_percentage":12,"resets_at":9999999999}}}},"updated_at":{now}}}"#
            ),
        );
        let old = AccountProfile {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            provider: "claude".to_string(),
            label: "Old name".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let mut data = UsageData::new(&UsageSelection { claude: Some(Scope::of_profile(&old)), codex: None }, items());
        data.refresh(false);
        wait_for_reads(&mut data);
        let groups = paddock_core::usage_scopes::bar_groups(&data.book, &|provider| items().provider(provider), now);
        assert_eq!(paddock_core::usage_scopes::group_summary(&groups[0]), "claude=Renamed:5h=12");
    }
    #[test]
    fn test_c_d75_a_terminal_link_ends_when_the_agent_exits() {
        let home = TempHome::new();
        let config = paddock_protocol::shared_config::config_dir();
        let id = "11111111-1111-4111-8111-111111111111";
        write(
            &config.join("account-profiles.json"),
            &format!(
                r#"{{"version":1,"profiles":[{{"id":"{id}","provider":"claude","label":"Main","runtime":"native"}}]}}"#
            ),
        );
        write(
            &config.join(format!("agent-profiles/{id}/claude/.claude.json")),
            r#"{"oauthAccount":{"accountUuid":"main-login"}}"#,
        );
        write(&home.dir.path().join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"main-login"}}"#);
        let mut data = UsageData::new(&UsageSelection::default(), items());
        data.observe(Some(1), None, Some("claude"), (Runtime::Native, None));
        let end = Instant::now() + Duration::from_secs(10);
        while data.book.selected("claude").and_then(|scope| scope.linked.clone()).is_none() && Instant::now() < end {
            data.tick(true, false);
            std::thread::sleep(Duration::from_millis(10));
        }
        let selected = data.book.selected("claude").cloned().unwrap();
        assert_eq!(selected.linked.as_ref().map(|linked| linked.label.as_str()), Some("Main"));
        assert_eq!(
            data.book.current(&selected).profile.map(|profile| profile.label),
            Some("Main".to_string()),
            "the bar names the saved account"
        );
        wait_for_reads(&mut data);
        data.observe(Some(1), None, Some("bash"), (Runtime::Native, None));
        assert!(data.book.selected("claude").unwrap().linked.is_none(), "the link is dropped once the agent is gone");
    }

    #[test]
    fn test_c_d75_opening_from_the_popup_selects_that_account() {
        let _home = TempHome::new();
        let account = AccountProfile {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            provider: "codex".to_string(),
            label: "Work".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let mut data = UsageData::new(&UsageSelection::default(), items());
        data.observe(Some(1), None, Some("bash"), (Runtime::Native, None));
        assert!(data.book.selected("codex").is_none());
        data.choose(Scope::of_profile(&account));
        assert_eq!(data.book.selected("codex").map(|scope| scope.profile.is_some()), Some(true));
        assert_eq!(
            data.selection_to_store().codex.and_then(|scope| scope.profile).map(|profile| profile.label),
            Some("Work".to_string()),
            "the choice is stored for the next start"
        );
        wait_for_reads(&mut data);
    }
}
