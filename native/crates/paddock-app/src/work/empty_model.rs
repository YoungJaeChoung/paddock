//! 작업 폴더가 없을 때 Work 목록에 보이는 첫 실행 안내의 문구. 화면을 모르는 순수 규칙이다.

/// 안내의 단계 하나. `command`가 있으면 단계 아래에 명령 글자로 따로 보인다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub text: &'static str,
    pub command: Option<&'static str>,
}

/// 첫 실행 안내 전체.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyState {
    pub title: &'static str,
    pub lead: &'static str,
    pub steps: Vec<Step>,
    pub note: &'static str,
}

/// 첫 실행 안내. 작업 폴더는 에이전트를 실행하면 생기므로 등록 단추 대신 그 다음 행동을 알려 준다.
///
/// 이미 열린 터미널(Unassigned 묶음)이 있으면 새로 열기를 첫 단계로 요구하지 않고 그 터미널을 쓰라고 한다.
///
/// Examples
/// --------
/// | has_unassigned | 첫 단계                                                           |
/// |----------------|-------------------------------------------------------------------|
/// | true           | `Use the terminal under Unassigned, or + for another.`            |
/// | false          | `Use + to open a terminal, or choose an environment with the arrow.` |
pub fn empty_state(has_unassigned: bool) -> EmptyState {
    let first = if has_unassigned {
        "Use the terminal under Unassigned, or + for another."
    } else {
        "Use + to open a terminal, or choose an environment with the arrow."
    };
    EmptyState {
        title: "Start a work session",
        lead: "No work folders yet.",
        steps: vec![
            Step { text: first, command: None },
            Step { text: "Go to your project in the terminal.", command: Some("cd /path/to/project") },
            Step { text: "Run claude or codex.", command: None },
        ],
        note: "Its terminals and files will appear here together.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_run_guide_has_the_old_apps_wording() {
        let guide = empty_state(true);
        assert_eq!(guide.title, "Start a work session");
        assert_eq!(guide.lead, "No work folders yet.");
        assert_eq!(guide.steps[0].text, "Use the terminal under Unassigned, or + for another.");
        assert_eq!(guide.steps[1].text, "Go to your project in the terminal.");
        assert_eq!(guide.steps[1].command, Some("cd /path/to/project"));
        assert_eq!(guide.steps[2].text, "Run claude or codex.");
        assert_eq!(guide.note, "Its terminals and files will appear here together.");
    }

    #[test]
    fn test_first_step_asks_for_a_new_terminal_only_when_none_is_open() {
        assert_eq!(
            empty_state(false).steps[0].text,
            "Use + to open a terminal, or choose an environment with the arrow."
        );
        assert_eq!(empty_state(true).steps.len(), empty_state(false).steps.len());
    }
}
