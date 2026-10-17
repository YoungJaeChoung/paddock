//! Work 목록 행에 보이는 에이전트 정보의 글: 오른쪽 프로그램 요약과 상태 글자. 화면 그리기를 모르는 순수 규칙이다.
use paddock_core::agent_account::scope_name;
use paddock_core::agent_model::{ActivityState, is_shell};
use paddock_protocol::message::TerminalState;

/// 행에 보이는 에이전트 상태 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// 화면 글자. 내부 이름(waiting·working·quiet)과 다르다.
    pub label: &'static str,
    /// 색을 고르는 상태. `Unknown`은 보이지 않으므로 여기 오지 않는다.
    pub activity: ActivityState,
}

/// 터미널 상태에서 행에 그릴 상태 글자를 고른다: Enter 직후는 `Sent`, 최근 출력은 `Output`, 조용하면 `Quiet`.
///
/// 에이전트가 아니거나(상태 없음) 앞쪽 프로그램을 아직 모르면(`Unknown`) 그리지 않는다.
/// 막 연 터미널은 대부분 곧 셸로 밝혀져 칸이 사라지므로, 잠깐 뜻 없는 글자가 깜빡이는 것을 막는다.
///
/// Examples
/// --------
/// | activity  | 결과     |
/// |-----------|----------|
/// | Waiting   | `Sent`   |
/// | Working   | `Output` |
/// | Quiet     | `Quiet`  |
/// | Unknown·없음 | 없음  |
pub fn status_of(state: Option<&TerminalState>) -> Option<Status> {
    let activity = state.and_then(|state| state.activity)?;
    let label = match activity {
        ActivityState::Waiting => Some("Sent"),
        ActivityState::Working => Some("Output"),
        ActivityState::Quiet => Some("Quiet"),
        ActivityState::Unknown => None,
    };
    label.map(|label| Status { label, activity })
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
    use paddock_core::agent_model::ActivityState;
    use paddock_protocol::message::TerminalState;

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
