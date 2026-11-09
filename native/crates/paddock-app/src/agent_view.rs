//! Work 목록 행에 보이는 에이전트 정보의 글: 오른쪽 프로그램 요약과 상태 글자. 화면 그리기를 모르는 순수 규칙이다.
use paddock_core::agent_account::scope_name;
use paddock_core::agent_model::{ActivityState, StatusState, is_shell};
use paddock_protocol::message::TerminalState;

/// 상태 칸의 색을 고르는 구분. 점·글자가 같은 색이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// 일하는 중(알린 `Working`)·최근 출력(추정 `Output`).
    Working,
    /// Enter를 막 보냄(추정 `Sent`)·사용자를 기다림(알린 `Needs input`).
    Attention,
    /// 알린 오류.
    Error,
    /// 쉬는 중·조용함.
    Quiet,
}

/// 행에 보이는 에이전트 상태 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// 화면 글자.
    pub label: &'static str,
    pub tone: Tone,
    /// 마우스를 올리면 뜨는 설명: `프로그램 — 뜻. 알린 것인지 추정인지`.
    pub tooltip: String,
}

/// 터미널 상태에서 행에 그릴 상태 글자를 고른다.
///
/// 에이전트가 직접 알린 상태(`signal`)가 있으면 그것이 우선이다: `Working`·`Needs input`·`Idle`(끝남도 같은 글자)·`Error`.
/// 없을 때만 터미널 입력·출력으로 추정한다: Enter 직후는 `Sent`, 최근 출력은 `Output`, 조용하면 `Quiet`.
/// 에이전트가 아니거나(상태 없음) 앞쪽 프로그램을 아직 모르면(`Unknown`) 그리지 않는다.
/// 막 연 터미널은 대부분 곧 셸로 밝혀져 칸이 사라지므로, 잠깐 뜻 없는 글자가 깜빡이는 것을 막는다.
///
/// Examples
/// --------
/// | 알린 상태   | 추정 상태 | 결과          |
/// |-------------|-----------|---------------|
/// | `working`   | 아무거나  | `Working`     |
/// | `blocked`   | 아무거나  | `Needs input` |
/// | `idle`·`done` | 아무거나 | `Idle`       |
/// | `error`     | 아무거나  | `Error`       |
/// | 없음        | Waiting   | `Sent`        |
/// | 없음        | Working   | `Output`      |
/// | 없음        | Quiet     | `Quiet`       |
/// | 어느 쪽이든 | Unknown·없음 | 없음       |
pub fn status_of(state: Option<&TerminalState>) -> Option<Status> {
    let state = state?;
    let activity = state.activity.filter(|activity| *activity != ActivityState::Unknown)?;
    let program = state.program.as_deref().unwrap_or("The agent");
    let reported = state.signal.as_ref().and_then(|signal| match signal.state {
        StatusState::Working => Some(("Working", Tone::Working, "The agent is working on a request.")),
        StatusState::Blocked => {
            Some(("Needs input", Tone::Attention, "The agent is waiting for your permission or an answer."))
        }
        StatusState::Done => {
            Some(("Idle", Tone::Quiet, "The agent finished its last request and is waiting for your next prompt."))
        }
        StatusState::Idle => Some(("Idle", Tone::Quiet, "The agent is waiting for your next prompt.")),
        StatusState::Error => Some(("Error", Tone::Error, "The agent stopped with an error.")),
        StatusState::Clear => None,
    });
    let status = match reported {
        Some((label, tone, detail)) => Status {
            label,
            tone,
            tooltip: format!("{program} \u{2014} {detail} Reported by {program} (program status)."),
        },
        None => {
            let (label, tone, detail) = match activity {
                ActivityState::Waiting => (
                    "Sent",
                    Tone::Attention,
                    "Enter was sent less than 3 seconds ago; no subsequent terminal output has been detected. This does not confirm the agent received a request.",
                ),
                ActivityState::Working => (
                    "Output",
                    Tone::Working,
                    "Terminal output was detected in the last 3 seconds; this can include prompt redraws. This does not confirm the agent is working.",
                ),
                ActivityState::Quiet | ActivityState::Unknown => (
                    "Quiet",
                    Tone::Quiet,
                    "No terminal output in the last 3 seconds. The agent may be waiting for input or still thinking.",
                ),
            };
            Status {
                label,
                tone,
                tooltip: format!("{program} \u{2014} {detail} Estimated from terminal input and output."),
            }
        }
    };
    Some(status)
}

/// 에이전트가 사용자의 허락이나 답을 기다린다고 알렸는가(알린 `blocked`). 접힌 폴더·묶음 행의 `N need input` 도움말이 센다.
pub fn needs_input(state: Option<&TerminalState>) -> bool {
    state.is_some_and(|state| {
        state.is_agent && state.signal.as_ref().is_some_and(|signal| signal.state == StatusState::Blocked)
    })
}

/// 입력을 기다리는 에이전트 수의 글. 없으면 None이다.
///
/// Examples
/// --------
/// | 수 | 결과           |
/// |----|----------------|
/// | 0  | 없음           |
/// | 1  | `1 needs input`|
/// | 3  | `3 need input` |
pub fn need_input_text(n_waiting: usize) -> Option<String> {
    match n_waiting {
        0 => None,
        1 => Some("1 needs input".to_string()),
        n => Some(format!("{n} need input")),
    }
}

/// 작업 폴더 행의 도움말.
pub fn folder_tip(
    _key: &str,
    _home: Option<&str>,
    _unseen: bool,
    _states: &[Option<TerminalState>],
) -> String {
    String::new()
}

/// Unassigned 묶음 행의 도움말.
pub fn group_tip(
    _root: Option<&TerminalState>,
    _home: Option<&str>,
    _unseen: bool,
    _members: &[Option<TerminalState>],
) -> String {
    String::new()
}

/// 터미널 탭의 도움말.
pub fn tab_tip(
    _label: &str,
    _state: Option<&TerminalState>,
    _detached: bool,
) -> String {
    String::new()
}

/// 행 이름 옆에 보이는 요약: `프로그램 · 계정 이름`.
///
/// 프로그램은 대화형 셸이거나 행 이름과 같으면 적지 않는다(이름이 이미 말한다). 계정 이름은 등록 계정·미등록 폴더로 실행한
/// 도구일 때만 붙고, 계정 터미널이 자기 이름(`Claude · Main`)을 행 이름으로 쓰면 반복하지 않는다.
///
/// Examples
/// --------
/// | 행 이름         | 프로그램 | 계정           | 결과                          |
/// |-----------------|----------|----------------|-------------------------------|
/// | `terminal 1`    | `claude` | 없음           | `claude`                      |
/// | `terminal 1`    | `bash`   | 없음           | (빈 글자)                     |
/// | `claude`        | `claude` | 없음           | (빈 글자)                     |
/// | `terminal 1`    | `claude` | 등록 계정 Main | `claude · Main`               |
/// | `Claude · Main` | `claude` | 등록 계정 Main | `claude`                      |
pub fn meta_text(
    row_name: &str,
    state: Option<&TerminalState>,
) -> String {
    let program = state.and_then(|state| state.program.as_deref());
    let program_part = program
        .filter(|program| !is_shell(Some(program)) && !row_name.eq_ignore_ascii_case(program))
        .unwrap_or_default();
    let account_part = state
        .and_then(|state| state.account.as_ref())
        .filter(|scope| scope.profile.is_some() || scope.custom || scope.linked.is_some())
        .map(|scope| (provider_title(&scope.provider), scope_name(Some(scope))))
        .filter(|(title, name)| format!("{title} · {name}") != row_name)
        .map(|(_, name)| name)
        .unwrap_or_default();
    [program_part, account_part.as_str()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// 상태 줄·계정 고르기에 보일 `도구 · 계정` 이름. 등록 계정·미등록 폴더로 실행한 도구이거나 계정 터미널이면 있고,
/// 일반 터미널의 기본 CLI나 에이전트가 없는 터미널은 빈 글자다(그때 호출자가 `Local shell` 같은 기본 글자를 보인다).
///
/// Examples
/// --------
/// | 범위                              | 결과                          |
/// |-----------------------------------|-------------------------------|
/// | 등록 계정 Main(claude)            | `Claude · Main`               |
/// | 미등록 폴더(codex)                | `Codex · Unregistered folder` |
/// | 기본 CLI(codex)·범위 없음         | (빈 글자)                     |
pub fn account_label(state: Option<&TerminalState>) -> String {
    state
        .and_then(|state| state.account.as_ref())
        .filter(|scope| scope.profile.is_some() || scope.custom || scope.linked.is_some())
        .map(|scope| format!("{} \u{b7} {}", provider_title(&scope.provider), scope_name(Some(scope))))
        .unwrap_or_default()
}

/// 상태 줄 왼쪽 글자: 계정 이름이 있으면 `도구 · 계정`, 없으면 `Local shell`.
pub fn status_scope_text(state: Option<&TerminalState>) -> String {
    let label = account_label(state);
    if label.is_empty() { "Local shell".to_string() } else { label }
}

/// 도구 이름의 화면 표기.
fn provider_title(provider: &str) -> &'static str {
    match provider {
        "claude" => "Claude",
        "codex" => "Codex",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_core::agent_account::{AccountProfile, Runtime, Scope};
    use paddock_core::agent_model::{ActivityState, StatusState};
    use paddock_protocol::message::{Signal, TerminalState};

    fn state(
        program: Option<&str>,
        activity: Option<ActivityState>,
    ) -> TerminalState {
        TerminalState {
            id: 1,
            cwd: None,
            program: program.map(str::to_string),
            is_agent: activity.is_some(),
            activity,
            finished: false,
            account: None,
            conversation: None,
            signal: None,
        }
    }

    #[test]
    fn test_status_label_names_the_three_visible_states_and_hides_unknown_and_non_agents() {
        let label = |activity| status_of(Some(&state(Some("claude"), activity))).map(|status| status.label);
        assert_eq!(label(Some(ActivityState::Working)), Some("Output"));
        assert_eq!(label(Some(ActivityState::Waiting)), Some("Sent"));
        assert_eq!(label(Some(ActivityState::Quiet)), Some("Quiet"));
        // 앞쪽 프로그램을 아직 모르는 순간(막 연 터미널)은 그리지 않는다. 대부분 곧 셸로 밝혀져 칸이 사라진다.
        assert_eq!(label(Some(ActivityState::Unknown)), None);
        assert_eq!(label(None), None);
        assert_eq!(status_of(None), None);
    }

    fn signalled(
        program: &str,
        state_name: StatusState,
    ) -> TerminalState {
        TerminalState {
            signal: Some(Signal { state: state_name, kind: None }),
            ..state(Some(program), Some(ActivityState::Quiet))
        }
    }

    #[test]
    fn test_c_a2_a_reported_state_wins_over_the_estimate_and_has_its_own_words() {
        let label = |state_name| status_of(Some(&signalled("claude", state_name))).map(|status| status.label);
        assert_eq!(label(StatusState::Working), Some("Working"));
        assert_eq!(label(StatusState::Blocked), Some("Needs input"));
        assert_eq!(label(StatusState::Idle), Some("Idle"));
        assert_eq!(label(StatusState::Done), Some("Idle"), "done is the same as idle on screen");
        assert_eq!(label(StatusState::Error), Some("Error"));
        // 추정 상태가 Output이어도 알린 상태가 있으면 알린 글자다.
        let mut recent_output = signalled("claude", StatusState::Idle);
        recent_output.activity = Some(ActivityState::Working);
        assert_eq!(status_of(Some(&recent_output)).map(|status| status.label), Some("Idle"));
        // 에이전트가 아니거나 프로그램을 모르면 신호가 있어도 보이지 않는다.
        let mut not_agent = signalled("bash", StatusState::Blocked);
        not_agent.activity = None;
        assert_eq!(status_of(Some(&not_agent)), None);
        let mut unknown = signalled("claude", StatusState::Blocked);
        unknown.activity = Some(ActivityState::Unknown);
        assert_eq!(status_of(Some(&unknown)), None);
    }

    #[test]
    fn test_c_a2_the_tooltip_says_what_the_state_means_and_whether_it_was_reported_or_estimated() {
        let reported = status_of(Some(&signalled("claude", StatusState::Blocked))).unwrap();
        assert_eq!(
            reported.tooltip,
            "claude \u{2014} The agent is waiting for your permission or an answer. Reported by claude (program status)."
        );
        let estimated = status_of(Some(&state(Some("codex"), Some(ActivityState::Working)))).unwrap();
        assert_eq!(
            estimated.tooltip,
            "codex \u{2014} Terminal output was detected in the last 3 seconds; this can include prompt redraws. This does not confirm the agent is working. Estimated from terminal input and output."
        );
        let sent = status_of(Some(&state(Some("codex"), Some(ActivityState::Waiting)))).unwrap();
        assert!(sent.tooltip.ends_with("Estimated from terminal input and output."), "{}", sent.tooltip);
    }

    #[test]
    fn test_c_a2_colors_follow_the_state_blocked_is_attention_and_error_is_the_danger_tone() {
        let tone = |state_name| status_of(Some(&signalled("claude", state_name))).map(|status| status.tone);
        assert_eq!(tone(StatusState::Working), Some(Tone::Working));
        assert_eq!(tone(StatusState::Blocked), Some(Tone::Attention));
        assert_eq!(tone(StatusState::Error), Some(Tone::Error));
        assert_eq!(tone(StatusState::Idle), Some(Tone::Quiet));
        assert_eq!(tone(StatusState::Done), Some(Tone::Quiet));
        let estimate = |activity| status_of(Some(&state(Some("claude"), Some(activity)))).map(|status| status.tone);
        assert_eq!(estimate(ActivityState::Working), Some(Tone::Working));
        assert_eq!(estimate(ActivityState::Waiting), Some(Tone::Attention));
        assert_eq!(estimate(ActivityState::Quiet), Some(Tone::Quiet));
    }

    #[test]
    fn test_c_a2_only_a_reported_blocked_agent_counts_as_needing_input() {
        assert!(needs_input(Some(&signalled("claude", StatusState::Blocked))));
        for state_name in [StatusState::Working, StatusState::Idle, StatusState::Done, StatusState::Error] {
            assert!(!needs_input(Some(&signalled("claude", state_name))), "{state_name:?}");
        }
        assert!(!needs_input(Some(&state(Some("claude"), Some(ActivityState::Quiet)))), "an estimate never counts");
        let mut left_for_shell = signalled("bash", StatusState::Blocked);
        left_for_shell.is_agent = false;
        assert!(!needs_input(Some(&left_for_shell)));
        assert!(!needs_input(None));
    }

    #[test]
    fn test_c_a2_the_folder_and_group_tooltip_counts_agents_that_need_input() {
        assert_eq!(need_input_text(0), None);
        assert_eq!(need_input_text(1).as_deref(), Some("1 needs input"));
        assert_eq!(need_input_text(2).as_deref(), Some("2 need input"));
    }

    fn at(
        program: &str,
        cwd: &str,
        state_name: Option<StatusState>,
    ) -> Option<TerminalState> {
        Some(TerminalState {
            cwd: Some(cwd.to_string()),
            signal: state_name.map(|state_name| Signal { state: state_name, kind: None }),
            ..state(Some(program), Some(ActivityState::Quiet))
        })
    }

    #[test]
    fn test_c_a2_the_folder_tip_shows_the_display_path_unseen_and_how_many_agents_need_input() {
        let waiting = at("claude", "/home/me/app", Some(StatusState::Blocked));
        let quiet = at("codex", "/home/me/app", Some(StatusState::Idle));
        let wsl = "\\\\wsl.localhost\\Ubuntu\\home\\me\\app";
        assert_eq!(folder_tip(wsl, None, false, &[]), "~/app");
        assert_eq!(folder_tip("/home/me/app", Some("/home/me"), false, &[quiet.clone(), None]), "~/app");
        assert_eq!(
            folder_tip("/srv/data", Some("/home/me"), true, &[waiting.clone()]),
            "/srv/data \u{b7} Unseen activity \u{b7} 1 needs input"
        );
        assert_eq!(
            folder_tip("/srv/data", None, false, &[waiting.clone(), waiting.clone(), quiet]),
            "/srv/data \u{b7} 2 need input"
        );
    }

    #[test]
    fn test_c_a2_the_group_tip_names_program_folder_unseen_and_waiting_agents() {
        let waiting = at("claude", "\\\\wsl.localhost\\Ubuntu\\home\\me\\app", Some(StatusState::Blocked));
        assert_eq!(
            group_tip(waiting.as_ref(), None, false, &[waiting.clone()]),
            "claude \u{b7} ~/app \u{b7} 1 needs input \u{2014} double-click to rename"
        );
        assert_eq!(
            group_tip(waiting.as_ref(), None, true, &[waiting.clone(), waiting.clone()]),
            "claude \u{b7} ~/app \u{b7} Unseen activity in this group \u{b7} 2 need input \u{2014} double-click to rename"
        );
        // 아직 상태를 모르는 터미널은 이름 바꾸기 안내만 남는다.
        assert_eq!(group_tip(None, None, false, &[None]), "double-click to rename");
    }

    #[test]
    fn test_c_a2_the_tab_tip_names_the_program_the_reported_state_and_a_tab_in_another_window() {
        let blocked = at("claude", "/work", Some(StatusState::Blocked));
        assert_eq!(tab_tip("claude", blocked.as_ref(), false), "claude \u{b7} claude \u{b7} Needs input");
        // 추정 상태는 탭에 보이지 않는다.
        let estimated = at("claude", "/work", None);
        assert_eq!(tab_tip("build", estimated.as_ref(), false), "build \u{b7} claude");
        assert_eq!(tab_tip("build", None, false), "build");
        // 다른 창에 있는 탭은 상태를 말하지 않는다.
        assert_eq!(tab_tip("build", blocked.as_ref(), true), "build \u{b7} Open in another window");
    }

    #[test]
    fn test_meta_shows_the_program_unless_it_is_a_shell_or_repeats_the_row_name() {
        assert_eq!(meta_text("terminal 1", Some(&state(Some("claude"), Some(ActivityState::Quiet)))), "claude");
        assert_eq!(meta_text("terminal 1", Some(&state(Some("bash"), None))), "", "a bare shell says nothing more");
        assert_eq!(meta_text("claude", Some(&state(Some("claude"), Some(ActivityState::Quiet)))), "");
        assert_eq!(meta_text("Claude", Some(&state(Some("claude"), None))), "", "names compare without case");
        assert_eq!(meta_text("build", Some(&state(Some("make"), None))), "make");
        assert_eq!(meta_text("terminal 1", None), "");
        assert_eq!(meta_text("terminal 1", Some(&state(None, None))), "");
    }

    #[test]
    fn test_meta_adds_the_account_name_when_it_differs_from_the_row_name() {
        let profile = AccountProfile {
            id: "a".to_string(),
            provider: "claude".to_string(),
            label: "Main".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let mut with_account = state(Some("claude"), Some(ActivityState::Quiet));
        with_account.account = Some(Scope::of_profile(&profile));
        assert_eq!(meta_text("terminal 1", Some(&with_account)), "claude · Main");
        assert_eq!(
            meta_text("Claude · Main", Some(&with_account)),
            "claude",
            "an account terminal's own name is not repeated"
        );
        // 기본 범위만 있는 에이전트는 계정 이름을 붙이지 않는다.
        let mut default_scope = state(Some("codex"), Some(ActivityState::Quiet));
        default_scope.account = Some(Scope::provider("codex"));
        assert_eq!(meta_text("terminal 1", Some(&default_scope)), "codex");
        let mut custom = state(Some("codex"), Some(ActivityState::Quiet));
        custom.account = Some(Scope::custom("codex"));
        assert_eq!(meta_text("terminal 1", Some(&custom)), "codex · Unregistered folder");
    }

    #[test]
    fn test_account_label_names_the_tool_and_account_only_for_registered_or_unregistered_scopes() {
        let profile = AccountProfile {
            id: "a".to_string(),
            provider: "claude".to_string(),
            label: "Main".to_string(),
            runtime: Runtime::Native,
            wsl_distribution: None,
        };
        let with = |scope| TerminalState { account: scope, ..state(Some("claude"), None) };
        assert_eq!(account_label(Some(&with(Some(Scope::of_profile(&profile))))), "Claude \u{b7} Main");
        assert_eq!(account_label(Some(&with(Some(Scope::custom("codex"))))), "Codex \u{b7} Unregistered folder");
        assert_eq!(account_label(Some(&with(Some(Scope::provider("codex"))))), "");
        assert_eq!(account_label(Some(&with(None))), "");
        assert_eq!(account_label(None), "");
        assert_eq!(status_scope_text(Some(&with(None))), "Local shell");
        assert_eq!(status_scope_text(Some(&with(Some(Scope::of_profile(&profile))))), "Claude \u{b7} Main");
    }
}
