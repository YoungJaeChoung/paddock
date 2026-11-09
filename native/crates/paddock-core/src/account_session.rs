//! 같은 대화를 다른 계정으로 이어 여는 일(`Continue with …`)과 빈 셸 탭에서 계정으로 시작하는 일(`Start in this terminal with …`)의 순수 규칙.
//!
//! 터미널이 어느 대화를 이어 가고 있는지는 데몬이 알려 주고(`TerminalState.conversation`), 여기서는 그 대화 파일을 다른 계정이 `--resume <경로>`로
//! 이어 열 수 있는지·어떤 메뉴를 보일지·어떤 문장으로 거부할지를 정한다. 대화 내용(JSONL 본문)은 읽지 않는다.
use crate::account_launch::LaunchError;
use crate::agent_account::{AccountProfile, Runtime};
use crate::agent_model::is_shell;

/// 대화 식별에 쓰는 한도와 문장.
pub struct SessionIdentity;

impl SessionIdentity {
    /// Claude가 작업 폴더 경로로 짓는 프로젝트 폴더 이름의 최대 글자 수. 넘으면 CLI가 이름을 줄이고 해시를 붙이므로 이름을 추측하지 않는다.
    pub const N_PROJECT_NAME_CHARS: usize = 200;
    /// 에이전트가 끝나기를 기다리는 최대 시간(ms).
    pub const STOP_TIMEOUT_MS: u64 = 10_000;
    /// 전체 탐색으로도 못 찾은 대화를 다시 훑기까지 기다리는 시간(ms). 훑기 한 번이 WSL 경로에서 수 초씩 걸린다.
    pub const SCAN_RETRY_MS: u64 = 5 * 60 * 1000;
    /// 새 계정 셸이 이어 연 대화를 확인하려고 다시 묻는 횟수와 간격(ms).
    pub const N_CONFIRM_ATTEMPTS: u32 = 20;
    pub const CONFIRM_INTERVAL_MS: u64 = 150;
    pub const MISSING_FILE: &'static str = "The saved conversation file is missing. Restore it before switching accounts; this terminal has not been changed.";
    pub const OTHER_FOLDER: &'static str = "This Claude runs with a different configuration folder than the selected account. Open it from its saved account to switch accounts.";
    pub const CODEX_UNAVAILABLE: &'static str = "Codex account switching in the same conversation is not available yet. Open the other account in a new terminal.";
    pub const SAME_ACCOUNT: &'static str = "This terminal already uses that account.";
    pub const OTHER_ENVIRONMENT: &'static str = "Continue with an account for the same CLI and environment.";
    pub const BUSY: &'static str = "Finish the other command in this terminal before switching accounts.";
    pub const BUSY_SHELL: &'static str =
        "A program is running in this terminal. Choose New terminal\u{2026} to open the account beside it.";
    pub const UNCONFIRMED: &'static str = "The account shell opened, but Claude has not confirmed the resumed conversation. Keep Claude open until it loads before switching again.";
    pub const ALREADY_SWITCHING: &'static str = "This terminal is already switching accounts. Wait for it to finish.";
    pub const NOT_CHECKED: &'static str = "This terminal could not be checked. Try again.";
    pub const NO_CONVERSATION: &'static str =
        "No conversation has been identified in this terminal. Start Claude Code before switching accounts.";
}

impl SessionIdentity {
    /// 새 터미널을 열지 못했을 때 사용자가 직접 같은 대화를 이어 열 명령 안내. 경로에 공백이 있으면 따옴표로 감싼다.
    ///
    /// Examples
    /// --------
    /// | 경로                      | 결과                                                    |
    /// |---------------------------|---------------------------------------------------------|
    /// | `/h/p/x.jsonl`            | `Run claude --resume /h/p/x.jsonl to continue.`         |
    /// | `C:\My Files\x.jsonl`    | `Run claude --resume "C:\My Files\x.jsonl" to continue.` |
    pub fn resume_hint(transcript_path: &str) -> String {
        let quoted = if transcript_path.contains(char::is_whitespace) {
            format!("\"{transcript_path}\"")
        } else {
            transcript_path.to_string()
        };
        format!("Run claude --resume {quoted} to continue.")
    }
}

/// `claude --resume`로 이어 열 대화. 같은 계정 폴더면 식별자만, 다른 계정이면 대화 파일의 경로다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resume {
    SessionId(String),
    TranscriptPath(String),
}

impl Resume {
    /// `--resume` 뒤에 붙일 값.
    pub fn argument(&self) -> &str {
        match self {
            Self::SessionId(value) | Self::TranscriptPath(value) => value,
        }
    }

    /// 값이 안전한지 본다: 식별자는 8-4-4-4-12 16진수, 경로는 절대 경로(`/…`·`C:\…`·`\\…`)이고 제어 문자가 없다.
    ///
    /// Examples
    /// --------
    /// | 값                                        | 결과                       |
    /// |-------------------------------------------|----------------------------|
    /// | `SessionId("0b9f2f77-…-…")`               | Ok                         |
    /// | `SessionId("../x")`                       | `InvalidSessionId`         |
    /// | `TranscriptPath("/home/a/projects/p/x.jsonl")` | Ok                    |
    /// | `TranscriptPath("relative/x.jsonl")`      | `InvalidResumePath`        |
    /// | `TranscriptPath("/a\nb")`                 | `InvalidResumePath`        |
    pub fn check(&self) -> Result<(), LaunchError> {
        match self {
            Self::SessionId(value) => crate::account_launch::check_session_id(value),
            Self::TranscriptPath(path) => {
                let absolute = path.starts_with('/')
                    || path.starts_with("\\\\")
                    || (path.len() > 2 && path.as_bytes()[1] == b':' && matches!(path.as_bytes()[2], b'\\' | b'/'));
                if absolute && !path.chars().any(|character| character < ' ' || character == '\u{7f}') {
                    Ok(())
                } else {
                    Err(LaunchError::InvalidResumePath)
                }
            }
        }
    }
}

/// Claude가 작업 폴더 경로로 짓는 프로젝트 폴더 이름: 영문자·숫자 밖의 글자는 모두 `-`가 된다. 200자를 넘으면 None(모름).
///
/// Examples
/// --------
/// | cwd                          | 결과                      |
/// |------------------------------|---------------------------|
/// | `/home/me/my project`        | `-home-me-my-project`     |
/// | `C:\Users\me\app`            | `C--Users-me-app`         |
/// | (201자 이상)                 | None                      |
/// | (빈 글자)                    | None                      |
pub fn claude_project_name(cwd: &str) -> Option<String> {
    let name: String =
        cwd.chars().map(|character| if character.is_ascii_alphanumeric() { character } else { '-' }).collect();
    (!name.is_empty() && name.chars().count() <= SessionIdentity::N_PROJECT_NAME_CHARS).then_some(name)
}

/// 대화가 기록될 곳으로 기대하는 파일 경로(`<설정 폴더>/projects/<프로젝트 이름>/<대화 ID>.jsonl`). 있는지는 호출자가 확인한다.
/// `separator`는 그 경로를 읽을 환경의 구분자다. 프로젝트 이름을 모르면 None이라 호출자가 전체 탐색에 맡긴다.
pub fn expected_transcript(
    config_dir: &str,
    session_id: &str,
    cwd: &str,
    separator: char,
) -> Option<String> {
    let project = claude_project_name(cwd)?;
    let base = config_dir.trim_end_matches(separator);
    Some(format!("{base}{separator}projects{separator}{project}{separator}{session_id}.jsonl"))
}

/// 이어 갈 대화의 출처: 터미널이 지금 실행하는 CLI의 도구·환경과 그 계정.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchSource {
    pub provider: String,
    pub runtime: Runtime,
    pub wsl_distribution: Option<String>,
    /// 지금 이 터미널의 계정. 등록 계정이 아니면 None이다.
    pub account_id: Option<String>,
}

/// 같은 도구·같은 실행 환경(WSL이면 같은 배포판, 이름은 대소문자 무관)의 계정만 대화를 이어 받을 수 있다.
fn same_environment(
    profile: &AccountProfile,
    source: &SwitchSource,
) -> bool {
    let distribution = |value: &Option<String>| value.as_deref().map(str::to_lowercase);
    profile.provider == source.provider
        && profile.runtime == source.runtime
        && (source.runtime != Runtime::Wsl
            || source.wsl_distribution.is_none()
            || distribution(&profile.wsl_distribution) == distribution(&source.wsl_distribution))
}

/// 대화를 이어 받을 수 있는 계정들(등록 순서 그대로).
pub fn switch_targets<'a>(
    profiles: &'a [AccountProfile],
    source: &SwitchSource,
) -> Vec<&'a AccountProfile> {
    profiles.iter().filter(|profile| same_environment(profile, source)).collect()
}

/// `target`으로 이어 열기를 시작해도 되는지. 안 되면 사용자에게 보일 문장이다.
///
/// Examples
/// --------
/// | 출처                | 대상                 | 결과                                                   |
/// |---------------------|----------------------|--------------------------------------------------------|
/// | claude · native · A | claude · native · B  | Ok                                                     |
/// | claude · native · A | claude · native · A  | `This terminal already uses that account.`             |
/// | claude · native     | claude · WSL         | `Continue with an account for the same CLI and …`      |
/// | codex               | codex                | `Codex account switching in the same conversation …`   |
pub fn check_continue(
    source: &SwitchSource,
    target: &AccountProfile,
) -> Result<(), &'static str> {
    if source.provider == "codex" {
        Err(SessionIdentity::CODEX_UNAVAILABLE)
    } else if source.account_id.as_deref() == Some(target.id.as_str()) {
        Err(SessionIdentity::SAME_ACCOUNT)
    } else if !same_environment(target, source) {
        Err(SessionIdentity::OTHER_ENVIRONMENT)
    } else {
        Ok(())
    }
}

/// 계정 고르기 메뉴가 하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchMode {
    /// 앞쪽이 Claude이고 대화를 알 때: 같은 대화를 다른 계정으로 이어 연다.
    Continue,
    /// 앞쪽이 빈 셸일 때: 이 탭에서 계정을 시작한다.
    Start,
    /// 그 밖: 새 터미널을 연다(기존 메뉴).
    New,
}

/// 보고 있는 터미널의 앞쪽 프로그램과 대화 여부로 메뉴가 하는 일을 정한다.
///
/// Examples
/// --------
/// | 앞쪽 프로그램 | 대화를 앎 | 결과       |
/// |---------------|-----------|------------|
/// | `claude`      | 예        | `Continue` |
/// | `claude`      | 아니오    | `New`      |
/// | `bash`        | -         | `Start`    |
/// | `codex`       | 예        | `New`      |
/// | (모름)        | -         | `New`      |
pub fn switch_mode(
    program: Option<&str>,
    has_conversation: bool,
) -> SwitchMode {
    if program == Some("claude") && has_conversation {
        SwitchMode::Continue
    } else if is_shell(program) {
        SwitchMode::Start
    } else {
        SwitchMode::New
    }
}

/// 메뉴 맨 위 안내 글. 고를 계정이 없으면 항목(새 대화·계정 관리)과 어긋나지 않게 없다는 사실을 적는다.
///
/// Examples
/// --------
/// | 모드       | 고를 계정 수 | 결과                                  |
/// |------------|--------------|---------------------------------------|
/// | `Continue` | 2            | `Continue this conversation with`     |
/// | `Start`    | 1            | `Start in this terminal`              |
/// | `New`      | 3            | `Open a new terminal`                 |
/// | (아무)     | 0            | `No accounts added yet`               |
pub fn menu_hint(
    mode: SwitchMode,
    n_choices: usize,
) -> &'static str {
    match (n_choices, mode) {
        (0, _) => "No accounts added yet",
        (_, SwitchMode::Continue) => "Continue this conversation with",
        (_, SwitchMode::Start) => "Start in this terminal",
        (_, SwitchMode::New) => "Open a new terminal",
    }
}

/// 항목 보조 글자: 지금 계정이면 `Current`, WSL 계정이면 `WSL`.
pub fn item_meta(
    is_current: bool,
    runtime: Runtime,
) -> &'static str {
    if is_current {
        "Current"
    } else if runtime == Runtime::Wsl {
        "WSL"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";

    fn account(
        id: &str,
        provider: &str,
        runtime: Runtime,
        distribution: Option<&str>,
    ) -> AccountProfile {
        AccountProfile {
            id: id.into(),
            provider: provider.into(),
            label: id.into(),
            runtime,
            wsl_distribution: distribution.map(str::to_string),
        }
    }

    fn source(
        provider: &str,
        runtime: Runtime,
        distribution: Option<&str>,
        id: Option<&str>,
    ) -> SwitchSource {
        SwitchSource {
            provider: provider.into(),
            runtime,
            wsl_distribution: distribution.map(str::to_string),
            account_id: id.map(str::to_string),
        }
    }

    #[test]
    fn test_c_d77_the_resume_hint_names_the_command_and_quotes_a_path_with_spaces() {
        assert_eq!(SessionIdentity::resume_hint("/h/p/x.jsonl"), "Run claude --resume /h/p/x.jsonl to continue.");
        assert_eq!(
            SessionIdentity::resume_hint("C:\\My Files\\x.jsonl"),
            "Run claude --resume \"C:\\My Files\\x.jsonl\" to continue."
        );
    }

    #[test]
    fn test_c_b05_the_project_folder_name_replaces_every_non_alphanumeric_and_gives_up_over_200_chars() {
        assert_eq!(claude_project_name("/home/me/my project").as_deref(), Some("-home-me-my-project"));
        assert_eq!(claude_project_name("C:\\Users\\me\\app").as_deref(), Some("C--Users-me-app"));
        assert_eq!(claude_project_name("/한글").as_deref(), Some("---"));
        assert_eq!(claude_project_name(&"a".repeat(200)).map(|name| name.len()), Some(200));
        assert_eq!(claude_project_name(&"a".repeat(201)), None);
        assert_eq!(claude_project_name(""), None);
    }

    #[test]
    fn test_c_b15_the_expected_transcript_follows_the_cli_layout_in_the_readers_separator() {
        assert_eq!(
            expected_transcript("/home/me/.claude/", SESSION, "/w/app", '/').as_deref(),
            Some(&*format!("/home/me/.claude/projects/-w-app/{SESSION}.jsonl"))
        );
        assert_eq!(
            expected_transcript("C:\\cfg", SESSION, "C:\\w", '\\').as_deref(),
            Some(&*format!("C:\\cfg\\projects\\C--w\\{SESSION}.jsonl"))
        );
        assert_eq!(expected_transcript("/c", SESSION, &"a".repeat(300), '/'), None);
    }

    #[test]
    fn test_c_resume_a_session_id_or_an_absolute_transcript_path_is_accepted_and_nothing_else() {
        assert_eq!(Resume::SessionId(SESSION.into()).check(), Ok(()));
        assert_eq!(Resume::SessionId("../x".into()).check(), Err(LaunchError::InvalidSessionId));
        for path in ["/home/a/p/x.jsonl", "C:\\a\\x.jsonl", "C:/a/x.jsonl", "\\\\wsl.localhost\\U\\x.jsonl"] {
            assert_eq!(Resume::TranscriptPath(path.into()).check(), Ok(()), "{path}");
        }
        for path in ["relative/x.jsonl", "", "/a\nb", "/a\u{7f}"] {
            assert_eq!(Resume::TranscriptPath(path.into()).check(), Err(LaunchError::InvalidResumePath), "{path:?}");
        }
        assert_eq!(Resume::TranscriptPath("/x".into()).argument(), "/x");
    }

    #[test]
    fn test_c_d77_switch_targets_keep_the_same_tool_and_environment_and_check_continue_names_the_reason() {
        let profiles = vec![
            account("a", "claude", Runtime::Native, None),
            account("b", "claude", Runtime::Native, None),
            account("c", "codex", Runtime::Native, None),
            account("d", "claude", Runtime::Wsl, Some("Ubuntu")),
            account("e", "claude", Runtime::Wsl, Some("Debian")),
        ];
        let ids =
            |source: &SwitchSource| switch_targets(&profiles, source).iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&source("claude", Runtime::Native, None, Some("a"))), ["a", "b"]);
        assert_eq!(
            ids(&source("claude", Runtime::Wsl, Some("ubuntu"), None)),
            ["d"],
            "the distribution name ignores case"
        );
        let native_a = source("claude", Runtime::Native, None, Some("a"));
        assert_eq!(check_continue(&native_a, &profiles[1]), Ok(()));
        assert_eq!(check_continue(&native_a, &profiles[0]), Err(SessionIdentity::SAME_ACCOUNT));
        assert_eq!(check_continue(&native_a, &profiles[3]), Err(SessionIdentity::OTHER_ENVIRONMENT));
        assert_eq!(
            check_continue(&source("codex", Runtime::Native, None, None), &profiles[2]),
            Err(SessionIdentity::CODEX_UNAVAILABLE)
        );
    }

    #[test]
    fn test_c_d77_the_menu_mode_follows_the_foreground_program_and_the_hint_says_what_a_choice_does() {
        assert_eq!(switch_mode(Some("claude"), true), SwitchMode::Continue);
        assert_eq!(switch_mode(Some("claude"), false), SwitchMode::New);
        assert_eq!(switch_mode(Some("bash"), false), SwitchMode::Start);
        assert_eq!(switch_mode(Some("codex"), true), SwitchMode::New);
        assert_eq!(switch_mode(None, false), SwitchMode::New);
        assert_eq!(menu_hint(SwitchMode::Continue, 2), "Continue this conversation with");
        assert_eq!(menu_hint(SwitchMode::Start, 1), "Start in this terminal");
        assert_eq!(menu_hint(SwitchMode::New, 3), "Open a new terminal");
        assert_eq!(menu_hint(SwitchMode::Continue, 0), "No accounts added yet");
        assert_eq!(item_meta(true, Runtime::Wsl), "Current");
        assert_eq!(item_meta(false, Runtime::Wsl), "WSL");
        assert_eq!(item_meta(false, Runtime::Native), "");
    }
}
