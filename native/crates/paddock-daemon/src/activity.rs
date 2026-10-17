//! 터미널마다 폴더·앞쪽 프로그램·에이전트 활동을 판정해 상태 알림(`TerminalState`)으로 만드는 곳.
//!
//! 앱을 닫아도 터미널은 데몬에 남으므로 활동 시각과 앞쪽 프로그램은 데몬이 쥐어야 다시 열 때 상태가 비지 않는다.
//! 터미널의 출력·입력 바이트는 데몬이 처음 보는 자리이고, 그 안의 OSC 7(폴더)·OSC 7501(에이전트 상태)도 여기서 읽는다.
//! 규칙(활동 상태·완료 추정·계정 판정)은 `paddock-core`의 순수 함수이고, 이 모듈은 터미널별 기록과 "언제 읽고 언제 보낼지"만 정한다.
//!
//! 비용 규칙: 프로세스 조사는 마지막 조사 뒤 출력이나 Enter가 있었던 터미널만 한다. 출력 없는 터미널은 조사 0회다.
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use paddock_core::agent_account::{
    AccountProfile, AttributeInput, Runtime, Scope, ScopeInput, attribute_account, running_scope,
};
use paddock_core::agent_model::{
    Activity, ProgramStatus, activity_state, is_agent, note_input, note_output, note_status, parse_program_status,
    program_name, settle,
};
use paddock_core::cwd_report::parse_cwd_report;
use paddock_core::osc_scan::{OscEvent, OscScanner};
use paddock_protocol::message::TerminalState;
use paddock_protocol::shared_config::{native_profile_directory, read_account_profiles};

use crate::history::AgentSeen;
use crate::probe::{Probe, Reading};
use crate::terminal::{Registry, Terminal};

/// 모든 터미널을 판정하는 틱 간격. 상태가 시간만으로 바뀌는 경우(Working → Quiet)는 최대 이만큼 늦게 알려진다.
pub const TICK_INTERVAL: Duration = Duration::from_secs(2);

/// 에이전트가 OSC 7501 지원 여부를 물었을 때 PTY에 써 주는 답. 같은 시퀀스를 그대로 돌려주는 것이 지원한다는 뜻이다.
pub const QUERY_REPLY: &[u8] = b"\x1b]7501;?\x1b\\";

/// 데몬이 시작된 뒤 지난 밀리초. 활동 판정의 모든 시각이 이 기준이다(시계가 거꾸로 가도 영향이 없다).
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// 조사 결과 중 상태에 쓰는 값.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observed {
    /// 운영체제에서 읽은 셸의 현재 폴더. 읽을 수 없는 환경(Windows 네이티브)이면 None이고, 그때는 셸이 알린 폴더(OSC 7)를 쓴다.
    pub cwd: Option<String>,
    /// 앞쪽 프로그램 이름.
    pub program: Option<String>,
    /// 앞쪽 프로그램이 claude·codex일 때 그 계정 범위.
    pub account: Option<Scope>,
    /// 앞쪽 프로그램이 claude·codex일 때 찾은 대화. 조사가 프로그램을 읽었는데 에이전트가 아니면 None이다.
    pub agent: Option<AgentSeen>,
}

/// 조사 원본(`Reading`)을 상태에 쓰는 값으로 바꾼다.
///
/// 환경으로 계정을 판정하는 것은 앞쪽 프로그램이 claude·codex일 때뿐이다. 환경을 읽지 못했으면(Windows 네이티브) 도구만 가리키는 기본 범위가 되고,
/// 계정 버튼으로 연 터미널(`launch`)이면 앞쪽이 셸일 때도 그 계정이 범위다. 계정 이름은 지금의 계정 목록(`profiles`)을 따른다.
pub fn observe(
    reading: &Reading,
    profiles: &[AccountProfile],
    launch: Option<&AccountProfile>,
) -> Observed {
    let program = program_name(&reading.argv);
    let detected = program.as_deref().and_then(|name| {
        let home = reading.env.as_ref().and_then(|env| env.home.clone()).unwrap_or_default();
        let profile_directory = |profile: &AccountProfile| match reading.runtime {
            Runtime::Native => native_profile_directory(profile),
            Runtime::Wsl => format!("{home}/.paddock/agent-profiles/{}/{}", profile.id, profile.provider),
        };
        // WSL 안의 경로는 이 컴퓨터의 파일 시스템에 없으므로 실제 경로를 따라가지 않는다.
        let real_path = |path: &str| match reading.runtime {
            Runtime::Native => std::fs::canonicalize(path)
                .map_or_else(|_| path.to_string(), |real| real.to_string_lossy().into_owned()),
            Runtime::Wsl => path.to_string(),
        };
        reading.env.as_ref().and_then(|env| {
            attribute_account(&AttributeInput {
                provider: name,
                runtime: reading.runtime,
                wsl_distribution: &reading.distribution,
                env,
                profiles,
                profile_directory: &profile_directory,
                default_directories: &[],
                real_path: &real_path,
            })
        })
    });
    // 이름을 바꾼 계정이 옛 이름으로 남지 않도록 목록의 최신 항목을 쓴다. 목록에서 빠진 계정은 연 때의 항목 그대로다.
    let launch_profile = launch.map(|opened| profiles.iter().find(|profile| profile.id == opened.id).unwrap_or(opened));
    let account =
        running_scope(&ScopeInput { program: program.as_deref(), launch_profile, detected: detected.as_ref() });
    Observed { cwd: reading.cwd.clone(), program, account, agent: None }
}

/// 터미널 하나의 활동·폴더·프로그램 기록.
#[derive(Debug)]
pub struct Tracker {
    activity: Activity,
    scanner: OscScanner,
    /// 셸이 OSC 7로 알린 현재 폴더.
    reported_cwd: Option<String>,
    observed: Observed,
    /// 마지막 조사 뒤 출력이나 Enter가 있었다.
    needs_probe: bool,
    /// 앞쪽 연결에 마지막으로 보낸(`finished`는 거짓으로 바꾼) 상태.
    last_sent: Option<TerminalState>,
    /// 앞쪽에서 도는 에이전트와 그 대화(터미널 기록에 남는다).
    agent: Option<AgentSeen>,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    /// 새 터미널의 기록. 셸이 무엇인지 알려면 한 번은 조사해야 하므로 첫 틱에 조사를 요청한다.
    pub fn new() -> Self {
        Self::opened_by(None)
    }

    /// 계정 버튼으로 연 터미널의 기록. 첫 조사 전에도 화면이 그 계정을 보이도록 계정 범위로 시작한다.
    pub fn opened_by(account: Option<&AccountProfile>) -> Self {
        Self {
            activity: Activity::idle(),
            scanner: OscScanner::default(),
            reported_cwd: None,
            observed: Observed { account: account.map(Scope::of_profile), ..Observed::default() },
            needs_probe: true,
            last_sent: None,
            agent: None,
        }
    }

    /// 터미널 출력을 기록한다. 출력 안의 OSC 7·OSC 7501도 읽는다.
    ///
    /// 에이전트가 OSC 7501 지원 여부를 물었으면 참을 돌려준다 — 호출자가 `QUERY_REPLY`를 PTY에 써야 에이전트가 상태를 알리기 시작한다.
    pub fn note_output(
        &mut self,
        data: &[u8],
        now: u64,
    ) -> bool {
        let mut query_seen = false;
        self.activity = note_output(&self.activity, now);
        self.needs_probe = true;
        for event in self.scanner.feed(data) {
            match event {
                OscEvent::Cwd(report) => {
                    let cwd = parse_cwd_report(&report);
                    self.reported_cwd = (!cwd.is_empty()).then_some(cwd);
                }
                OscEvent::ProgramStatus(body) => match parse_program_status(&body) {
                    Some(ProgramStatus::Query) => query_seen = true,
                    Some(status) => self.activity = note_status(&self.activity, &status, now),
                    None => {}
                },
                OscEvent::Title(_) => {}
            }
        }
        query_seen
    }

    /// 사용자 입력을 기록한다. Enter(`\r`)가 들어오면 프로그램이 바뀌었을 수 있어 조사를 요청한다.
    pub fn note_input(
        &mut self,
        data: &[u8],
        now: u64,
    ) {
        self.activity = note_input(&self.activity, data, now);
        if data.contains(&b'\r') {
            self.needs_probe = true;
        }
    }

    /// 조사가 필요한지 묻고 요청을 지운다. 지난 조사 뒤 출력이나 Enter가 없었으면 거짓이다.
    pub fn take_probe_request(&mut self) -> bool {
        std::mem::take(&mut self.needs_probe)
    }

    /// 조사 결과를 반영하고, 폴더·앞쪽 프로그램·대화 중 바뀐 것이 있으면 true를 돌려준다. 읽지 못한 값(None)은 이전 값을 지우지 않는다.
    pub fn apply_probe(
        &mut self,
        observed: Observed,
    ) -> bool {
        let before = (self.cwd().map(str::to_string), self.observed.program.clone(), self.agent.clone());
        if observed.cwd.is_some() {
            self.observed.cwd = observed.cwd;
        }
        if observed.program.is_some() {
            self.observed.program = observed.program;
            self.observed.account = observed.account;
            self.agent = observed.agent;
        }
        before != (self.cwd().map(str::to_string), self.observed.program.clone(), self.agent.clone())
    }

    /// 지금 앞쪽 프로그램의 이름. 아직 조사하지 못했으면 None이다.
    pub fn program(&self) -> Option<&str> {
        self.observed.program.as_deref()
    }

    /// 앞쪽 에이전트의 대화. 대화를 아직 못 찾았거나 에이전트가 아니면 None이다.
    pub fn agent(&self) -> Option<&AgentSeen> {
        self.agent.as_ref()
    }

    /// 지금 폴더: 운영체제에서 읽은 값, 없으면 셸이 알린 값(OSC 7).
    pub fn cwd(&self) -> Option<&str> {
        self.observed.cwd.as_deref().or(self.reported_cwd.as_deref())
    }

    fn state_of(
        &self,
        id: u32,
        now: u64,
        agent_now: Option<bool>,
    ) -> TerminalState {
        TerminalState {
            id,
            cwd: self.cwd().map(str::to_string),
            program: self.observed.program.clone(),
            is_agent: agent_now == Some(true),
            activity: activity_state(&self.activity, now, agent_now),
            finished: false,
            account: self.observed.account.clone(),
        }
    }

    /// 지금의 상태. 보낸 것으로 기록하지 않는다(목록 응답용).
    pub fn state_now(
        &self,
        id: u32,
        now: u64,
    ) -> TerminalState {
        self.state_of(id, now, self.observed.program.as_deref().map(is_agent))
    }

    /// 연결이 터미널에 막 붙었을 때 보낼 지금의 상태. 보낸 것으로 기록해, 바뀌지 않은 값을 곧바로 다시 보내지 않는다.
    pub fn snapshot(
        &mut self,
        id: u32,
        now: u64,
    ) -> TerminalState {
        let state = self.state_now(id, now);
        self.last_sent = Some(state.clone());
        state
    }

    /// 시간을 반영해 판정하고, 앞쪽 연결에 보낼 프레임이 있으면 돌려준다.
    ///
    /// 보내는 경우는 둘이다: 마지막으로 보낸 값과 달라졌을 때, 에이전트의 긴 작업이 끝났을 때(`finished`는 그 한 프레임에서만 참).
    /// 아무것도 달라지지 않았으면 None이라 같은 값을 반복해 보내지 않는다.
    pub fn evaluate(
        &mut self,
        id: u32,
        now: u64,
    ) -> Option<TerminalState> {
        let agent_now = self.observed.program.as_deref().map(is_agent);
        let settled = settle(&self.activity, now, agent_now);
        self.activity = settled.activity;
        let state = self.state_of(id, now, agent_now);
        let changed = self.last_sent.as_ref() != Some(&state);
        if changed {
            self.last_sent = Some(state.clone());
        }
        if settled.finished {
            Some(TerminalState { finished: true, ..state })
        } else if changed {
            Some(state)
        } else {
            None
        }
    }
}

/// 모든 터미널을 한 번 판정한다: 조사가 필요한 터미널을 모아 한 번에 읽고, 결과를 반영한 뒤, 바뀐 상태를 보낸다.
///
/// 조사는 어떤 잠금도 쥐지 않은 채 한다(WSL 조사는 수백 ms 걸린다). 조사할 터미널이 없으면 프로세스 목록도 읽지 않는다.
pub fn tick(
    registry: &Registry,
    probe: &dyn Probe,
    now: u64,
) {
    let terminals = registry.all();
    let targets: Vec<_> = terminals.iter().filter_map(|terminal| terminal.take_probe_request()).collect();
    let readings = if targets.is_empty() { Default::default() } else { probe.read(&targets) };
    apply_readings(&terminals, &readings, now);
    for terminal in &terminals {
        terminal.evaluate_and_send(now);
    }
}

/// 기록을 쓰기 직전에 이 터미널들을 지금 바로 한 번에 조사해 폴더·앞쪽 프로그램·대화를 최신으로 만든다.
///
/// 틱의 조사는 출력이나 Enter가 있었을 때만 일어나므로, 출력 없이 바뀐 것(늦게 생긴 대화 기록)은 쓰는 쪽이 직접 읽어 와야 한다.
/// 터미널마다 따로 읽으면 프로세스 목록·WSL 호출이 터미널 수만큼 반복되므로 틱처럼 모아서 한 번 읽는다.
pub fn refresh_terminals(
    terminals: &[Arc<Terminal>],
    probe: &dyn Probe,
    now: u64,
) {
    let targets: Vec<_> = terminals.iter().filter_map(|terminal| terminal.probe_target()).collect();
    if !targets.is_empty() {
        let readings = probe.read(&targets);
        apply_readings(terminals, &readings, now);
    }
}

/// 조사 결과를 터미널마다 반영한다.
fn apply_readings(
    terminals: &[Arc<Terminal>],
    readings: &BTreeMap<u32, Reading>,
    now: u64,
) {
    // 계정 목록은 에이전트가 보이거나 계정 버튼으로 연 터미널을 조사할 때만 읽는다.
    let needs_profiles = terminals.iter().any(|terminal| {
        readings.get(&terminal.id).is_some_and(|reading| {
            terminal.account().is_some() || program_name(&reading.argv).is_some_and(|name| is_agent(&name))
        })
    });
    let profiles = if needs_profiles { read_account_profiles() } else { Vec::new() };
    for terminal in terminals {
        if let Some(reading) = readings.get(&terminal.id) {
            let mut observed = observe(reading, &profiles, terminal.account());
            observed.agent = terminal.find_agent(reading, observed.program.as_deref());
            terminal.apply_observed(observed, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_core::agent_account::{AccountEnv, AccountProfile, Runtime};
    use paddock_core::agent_model::{ActivityState, limit};
    use paddock_core::agent_resume::AgentConversation;

    const ID: u32 = 1;
    const SHELL_PROMPT: &[u8] = b"user@host:~$ ";

    fn observed(program: &str) -> Observed {
        Observed { cwd: None, program: Some(program.to_string()), account: None, agent: None }
    }

    /// 프롬프트가 한 번 나온 뒤 조사가 셸을 확인한 터미널.
    fn shell_terminal() -> Tracker {
        let mut tracker = Tracker::new();
        tracker.note_output(SHELL_PROMPT, 0);
        assert!(tracker.take_probe_request());
        tracker.apply_probe(observed("bash"));
        tracker.evaluate(ID, 0);
        tracker
    }

    #[test]
    fn test_c_d37_state_sent_only_on_change() {
        let mut tracker = shell_terminal();
        // 같은 출력이 얼마나 반복돼도, 셸이라 보이는 상태가 그대로이면 프레임이 없다.
        for now in (100..5000).step_by(100) {
            tracker.note_output(b"same line\r\n", now);
            assert_eq!(tracker.evaluate(ID, now), None, "no change at {now}");
        }
        // 폴더가 바뀌면 한 번만 나간다.
        tracker.note_output(b"\x1b]7;file://host/home/me/app\x07", 5100);
        let changed = tracker.evaluate(ID, 5100).expect("cwd changed");
        assert_eq!(changed.cwd.as_deref(), Some("/home/me/app"));
        assert!(!changed.finished);
        assert_eq!(tracker.evaluate(ID, 5200), None);
    }

    #[test]
    fn test_c_d8a_idle_terminals_cost_no_probe() {
        let mut trackers: Vec<Tracker> = (0..4).map(|_| shell_terminal()).collect();
        // 출력도 Enter도 없는 60초: 2초 틱 30번 동안 조사 요청은 0이다.
        let mut n_probes = 0;
        for tick in 1..=30u64 {
            for tracker in &mut trackers {
                if tracker.take_probe_request() {
                    n_probes += 1;
                }
                assert_eq!(tracker.evaluate(ID, tick * 2000), None);
            }
        }
        assert_eq!(n_probes, 0);
        // 출력이나 Enter가 한 번 있으면 다음 틱에 한 번만 조사한다.
        trackers[0].note_output(b"x", 61_000);
        trackers[1].note_input(b"ls\r", 61_000);
        trackers[2].note_input(b"typing", 61_000);
        let asked: Vec<bool> = trackers.iter_mut().map(Tracker::take_probe_request).collect();
        assert_eq!(asked, vec![true, true, false, false]);
        assert!(!trackers[0].take_probe_request(), "asking clears the request");
    }

    #[test]
    fn test_a_new_terminal_is_probed_once_even_before_any_output() {
        let mut tracker = Tracker::new();
        assert!(tracker.take_probe_request());
        assert!(!tracker.take_probe_request());
    }

    #[test]
    fn test_c_d37_osc7501_query_is_reported_so_the_daemon_can_answer() {
        let mut tracker = shell_terminal();
        assert!(!tracker.note_output(b"plain output", 10));
        assert!(tracker.note_output(b"\x1b]7501;?\x1b\\", 20));
        // 조각 경계에 걸려도 한 번만 묻는다.
        assert!(!tracker.note_output(b"\x1b]75", 30));
        assert!(tracker.note_output(b"01;?\x07", 40));
        // 상태 알림은 답할 필요가 없다.
        assert!(!tracker.note_output(b"\x1b]7501;state=working\x07", 50));
    }

    #[test]
    fn test_an_agent_run_shows_waiting_working_quiet_and_announces_the_finish_once() {
        let mut tracker = shell_terminal();
        tracker.note_input(b"claude\r", 1000);
        tracker.apply_probe(observed("claude"));
        let waiting = tracker.evaluate(ID, 1100).expect("agent detected");
        assert_eq!((waiting.is_agent, waiting.activity), (true, Some(ActivityState::Waiting)));
        // 6초 동안 출력이 이어지는 일.
        let mut working_frames = 0;
        for now in (1500..=7000).step_by(500) {
            tracker.note_output(b"thinking...\r", now);
            if let Some(state) = tracker.evaluate(ID, now) {
                working_frames += 1;
                assert_eq!(state.activity, Some(ActivityState::Working));
                assert!(!state.finished);
            }
        }
        assert_eq!(working_frames, 1, "Working is sent once, not at every chunk");
        // 출력이 멎는다.
        assert_eq!(tracker.evaluate(ID, 7000 + 1000), None);
        let done = tracker.evaluate(ID, 7000 + limit::QUIET_MS).expect("quiet and finished");
        assert!(done.finished);
        assert_eq!(done.activity, Some(ActivityState::Quiet));
        assert_eq!(tracker.evaluate(ID, 7000 + limit::QUIET_MS + 2000), None, "finish is announced once");
    }

    #[test]
    fn test_a_blocked_report_from_the_agent_announces_at_once() {
        let mut tracker = shell_terminal();
        tracker.apply_probe(observed("claude"));
        tracker.note_output(b"\x1b]7501;state=working\x07", 100);
        tracker.evaluate(ID, 100);
        tracker.note_output(b"\x1b]7501;state=blocked:kind=permission\x07", 1000);
        let state = tracker.evaluate(ID, 1000).expect("blocked");
        assert!(state.finished);
    }

    #[test]
    fn test_snapshot_is_the_current_state_and_counts_as_sent() {
        let mut tracker = shell_terminal();
        tracker.note_output(b"\x1b]7;/work\x07", 10);
        let snapshot = tracker.snapshot(ID, 10);
        assert_eq!(snapshot.cwd.as_deref(), Some("/work"));
        assert_eq!(snapshot.program.as_deref(), Some("bash"));
        assert_eq!(tracker.evaluate(ID, 10), None, "a client that just got the snapshot needs nothing more");
    }

    #[test]
    fn test_probe_cwd_wins_over_the_reported_one() {
        let mut tracker = shell_terminal();
        tracker.note_output(b"\x1b]7;C:\\reported\x07", 10);
        assert_eq!(tracker.snapshot(ID, 10).cwd.as_deref(), Some("C:/reported"));
        tracker.apply_probe(Observed { cwd: Some("/proc/cwd".to_string()), ..observed("bash") });
        assert_eq!(tracker.snapshot(ID, 20).cwd.as_deref(), Some("/proc/cwd"));
    }

    fn reading(
        argv: &[&str],
        env: Option<AccountEnv>,
    ) -> Reading {
        Reading {
            cwd: Some("/work".to_string()),
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            env,
            runtime: Runtime::Native,
            distribution: String::new(),
            pid: None,
            started_at: None,
            session_record: None,
        }
    }

    #[test]
    fn test_c_d40_a_probe_that_reads_a_shell_clears_the_agent_but_a_failed_probe_keeps_it() {
        let mut tracker = shell_terminal();
        let seen = AgentSeen {
            pid: 9,
            conversation: paddock_core::agent_resume::AgentConversation {
                provider: "claude".into(),
                session_id: "s".into(),
                cwd: "/w".into(),
                config_dir: None,
            },
        };
        tracker.apply_probe(Observed { agent: Some(seen.clone()), ..observed("claude") });
        assert_eq!(tracker.agent(), Some(&seen));
        // 읽지 못한 조사(프로그램 None)는 지난 값을 지우지 않는다.
        tracker.apply_probe(Observed::default());
        assert_eq!(tracker.agent(), Some(&seen));
        // 셸로 돌아온 것을 읽으면 대화는 더 이을 것이 없다.
        tracker.apply_probe(observed("bash"));
        assert_eq!(tracker.agent(), None);
    }

    #[test]
    fn test_observe_names_the_program_and_keeps_the_probed_cwd() {
        let observed =
            observe(&reading(&["node", "/usr/lib/node_modules/@openai/codex/bin/codex.js"], None), &[], None);
        assert_eq!(observed.program.as_deref(), Some("codex"));
        assert_eq!(observed.cwd.as_deref(), Some("/work"));
        let shell = observe(&reading(&["-bash"], None), &[], None);
        assert_eq!((shell.program.as_deref(), shell.account), (Some("bash"), None));
    }

    fn claude_profile() -> AccountProfile {
        AccountProfile {
            id: "11111111-1111-4111-8111-111111111111".to_string(),
            provider: "claude".to_string(),
            label: "Work Claude".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        }
    }

    fn account_of(
        runtime: Runtime,
        config_dir: &str,
        profile: &AccountProfile,
    ) -> Scope {
        let env = AccountEnv {
            home: Some("/home/me".to_string()),
            claude_config_dir: Some(config_dir.to_string()),
            ..AccountEnv::default()
        };
        let reading = Reading { runtime, distribution: "Ubuntu".to_string(), ..reading(&["claude"], Some(env)) };
        observe(&reading, std::slice::from_ref(profile), None).account.expect("a scope")
    }

    #[test]
    fn test_observe_attributes_the_account_of_a_wsl_agent_from_its_environment() {
        let profile =
            AccountProfile { runtime: Runtime::Wsl, wsl_distribution: Some("Ubuntu".to_string()), ..claude_profile() };
        let directory = format!("/home/me/.paddock/agent-profiles/{}/claude", profile.id);
        let account = account_of(Runtime::Wsl, &directory, &profile);
        assert_eq!(account.profile.as_ref().map(|profile| profile.label.as_str()), Some("Work Claude"));
        // 다른 설정 폴더는 미등록 폴더다.
        assert!(account_of(Runtime::Wsl, "/tmp/other", &profile).custom);
    }

    #[cfg(unix)]
    #[test]
    fn test_observe_attributes_the_account_of_a_native_agent_from_its_environment() {
        // Windows 네이티브는 프로세스 환경을 읽지 않으므로 이 판정은 Unix에서만 쓰인다.
        let profile = claude_profile();
        let directory = paddock_protocol::shared_config::native_profile_directory(&profile);
        let account = account_of(Runtime::Native, &directory, &profile);
        assert_eq!(account.profile.as_ref().map(|profile| profile.label.as_str()), Some("Work Claude"));
        assert!(account_of(Runtime::Native, "/tmp/other", &profile).custom);
    }

    #[test]
    fn test_observe_leaves_an_agent_without_a_readable_environment_in_the_default_scope() {
        let unread = observe(&reading(&["claude"], None), &[claude_profile()], None).account.expect("a scope");
        assert_eq!((unread.provider.as_str(), unread.profile.is_none(), unread.custom), ("claude", true, false));
    }

    #[test]
    fn test_observe_keeps_the_account_that_opened_the_terminal_while_a_shell_is_in_front_and_follows_a_rename() {
        let opened = claude_profile();
        let shell = observe(&reading(&["-bash"], None), &[], Some(&opened));
        assert_eq!(
            shell.account.and_then(|scope| scope.profile).map(|profile| profile.label),
            Some("Work Claude".into())
        );
        // 목록에서 이름이 바뀌었으면 최신 이름이 보이고, 목록에서 빠졌으면 연 때의 항목이 남는다.
        let renamed = AccountProfile { label: "Renamed".to_string(), ..claude_profile() };
        let after = observe(&reading(&["-bash"], None), &[renamed], Some(&opened));
        assert_eq!(after.account.and_then(|scope| scope.profile).map(|profile| profile.label), Some("Renamed".into()));
        // 처음 조사 전에도 계정 범위로 시작한다.
        let tracker = Tracker::opened_by(Some(&opened));
        assert_eq!(tracker.state_now(1, 0).account.map(|scope| scope.provider), Some("claude".to_string()));
        assert_eq!(Tracker::new().state_now(1, 0).account, None);
    }

    #[test]
    fn test_c_d46_a_probe_reports_whether_folder_program_or_conversation_changed() {
        let mut tracker = shell_terminal();
        let seen = AgentSeen {
            pid: 42,
            conversation: AgentConversation {
                provider: "claude".into(),
                session_id: "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10".into(),
                cwd: "/work".into(),
                config_dir: None,
            },
        };
        assert!(!tracker.apply_probe(observed("bash")), "the same shell again changes nothing");
        assert!(tracker.apply_probe(observed("claude")), "a new foreground program");
        assert!(
            tracker.apply_probe(Observed { agent: Some(seen.clone()), ..observed("claude") }),
            "the conversation was found"
        );
        assert!(!tracker.apply_probe(Observed { agent: Some(seen), ..observed("claude") }), "nothing new");
        assert!(tracker.apply_probe(Observed { cwd: Some("/proc/cwd".to_string()), ..observed("claude") }));
    }
}
