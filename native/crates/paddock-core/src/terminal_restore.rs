//! 터미널 프로세스째 사라진 터미널을 어떻게 다시 여는지 정하는 순수 규칙.
//!
//! 저장해 둔 기록(화면·셸·폴더·에이전트 대화)에서 새 터미널의 실행 방식, 새 셸에 입력할 이어하기 명령, 화면에 보일 안내 문구를 만든다.
use crate::account_launch::{Launch, quote_posix};
use crate::shell_catalog::wsl_folder;
use crate::terminal_history::HistoryRecord;

/// 되살리기 규칙의 상수.
pub struct RestoreRules;

impl RestoreRules {
    /// 환경 변수 앞붙임 문법(`NAME=값 명령`)을 쓸 수 없는 셸 실행 파일 이름.
    pub const NON_POSIX_SHELLS: [&'static str; 6] =
        ["powershell", "powershell.exe", "pwsh", "pwsh.exe", "cmd", "cmd.exe"];
    /// 새 셸이 프롬프트를 낸 것으로 보는 출력 멎음 시간(ms).
    pub const PROMPT_QUIET_MS: u64 = 500;
    /// 프롬프트를 기다리는 최대 시간(ms).
    pub const PROMPT_TIMEOUT_MS: u64 = 8000;
    /// 복원한 화면 아래에 쓰는 안내의 기본 문장.
    pub const BASE_NOTICE: &'static str =
        "Previous shell ended. Screen restored; commands that were running were stopped.";
    /// 이전 터미널의 프로그램이 새 터미널에서 되살린 화면 뒤에 남기지 않도록 끄는 터미널 모드: 대체 화면, 마우스 보고, 붙여넣기 감싸기,
    /// 확장 키보드, 숨긴 커서, 선 그리기 문자 집합(G0를 ASCII로), 스크롤 영역, 방향키의 응용 프로그램 모드.
    /// 스크롤 영역을 풀면 커서가 화면 맨 위로 가므로 커서 저장·복원(`ESC 7`·`ESC 8`)으로 감싸 이전 화면 끝에 커서를 남긴다.
    pub const MODE_RESET: &'static str = "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[<99u\x1b[?25h\x1b(B\x1b7\x1b[r\x1b8\x1b[?1l";
}

/// 새 터미널을 어떻게 여는가.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreTarget {
    /// 일반 터미널: 기록의 셸 이름·배포판으로, 시작 폴더는 `cwd`로 연다.
    Shell { shell: String, wsl_distribution: Option<String>, cwd: Option<String> },
    /// 계정 터미널: 계정 터미널을 다시 만든다. `resume_session_id`가 있으면 그 Claude 대화를 이어 연다.
    /// 계정을 더 쓸 수 없으면 `launch`(기록된 실행 명세)로 대신 연다.
    Account { launch: Launch, resume_session_id: Option<String> },
}

/// 되살리기 계획.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestorePlan {
    pub target: RestoreTarget,
    /// 새 셸이 뜬 뒤 입력할 이어하기 명령. 없으면 None.
    pub command: Option<String>,
    /// 복원된 화면 아래에 보일 한 줄 안내.
    pub notice: String,
}

/// 이어하기 명령을 입력할 셸이 `NAME=값 명령` 문법을 쓰는 POSIX 계열(bash·zsh·fish·sh·WSL)인지.
///
/// Examples
/// --------
/// | 셸                                   | 결과  |
/// |--------------------------------------|-------|
/// | `bash` · `wsl` · `/bin/zsh`          | true  |
/// | `powershell` · `C:\...\pwsh.exe` · `cmd` | false |
pub fn is_posix_shell(shell: &str) -> bool {
    let name = shell.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    !RestoreRules::NON_POSIX_SHELLS.contains(&name.as_str())
}

/// 이어하기 명령. 모르는 도구면 None이다. 대화 식별자는 UUID로 검증한 값만 들어온다.
fn resume_command(
    provider: &str,
    session_id: &str,
) -> Option<String> {
    match provider {
        "claude" => Some(format!("claude --resume {session_id}")),
        "codex" => Some(format!("codex resume {session_id}")),
        _ => None,
    }
}

/// 도구 이름의 표시 글자.
fn display_name(provider: &str) -> &'static str {
    if provider == "claude" { "Claude" } else { "Codex" }
}

/// 저장 기록으로 새 터미널의 실행 방식과 이어하기를 정한다.
///
/// - 계정 터미널(`launch`에 계정이 있는 것)이고 Claude 대화가 있으면 그 대화를 이어 열도록 계획한다. 시작 폴더는 처음 폴더를 유지한다.
/// - 일반 터미널은 기록의 셸·배포판으로 열고 시작 폴더를 기록의 현재 폴더로 바꾼다(기록이 없으면 처음 폴더).
///   WSL 공유 경로(`\\wsl.localhost\...`)의 폴더는 그 배포판과 Linux 경로로 나눈다. 대화가 있으면 새 셸이 뜬 뒤 입력할 명령을 돌려준다 —
///   사용자 셸의 PATH·별칭을 그대로 쓰도록 직접 입력한다.
/// - 에이전트가 떠 있었는데 대화를 확신할 수 없으면 이어하지 않고 안내만 한다.
///
/// Examples
/// --------
/// | 기록의 agent                    | 터미널 종류 | 결과                                               |
/// |---------------------------------|-------------|----------------------------------------------------|
/// | claude, 대화 있음               | 일반        | `command: claude --resume <id>`                    |
/// | claude, 대화 있음               | 계정        | 대화를 이어 여는 계정 터미널, `command: None`      |
/// | codex, 대화 있음                | 일반        | `command: codex resume <id>`                       |
/// | codex, 대화 있음                | 계정        | 이어하지 않음, 안내에 `codex resume`               |
/// | None, 앞쪽 프로그램이 codex     | 일반        | 이어하지 않음, 안내에 `codex resume`               |
/// | None, 셸만                      | 일반        | 이어하지 않음, 안내는 화면 복원만                  |
pub fn plan_restore(record: &HistoryRecord) -> RestorePlan {
    let agent = record.agent.as_ref().filter(|agent| resume_command(&agent.provider, &agent.session_id).is_some());
    let account_launch = record.launch.as_ref().filter(|launch| launch.account.is_some());
    let mut command = None;
    let mut resumed_with = None;
    let target = match account_launch {
        Some(launch) => {
            let resume = agent.filter(|agent| agent.provider == "claude");
            resumed_with = resume;
            RestoreTarget::Account {
                launch: launch.clone(),
                resume_session_id: resume.map(|agent| agent.session_id.clone()),
            }
        }
        None => {
            let folder = Some(record.cwd_now.as_str()).filter(|cwd| !cwd.is_empty()).or(record.cwd_start.as_deref());
            let in_wsl_share = folder.filter(|_| record.shell == "wsl").and_then(wsl_folder);
            let (wsl_distribution, cwd) = match in_wsl_share {
                Some((distribution, path)) => (Some(distribution), Some(path)),
                None => (record.wsl_distribution.clone(), folder.map(str::to_string)),
            };
            if let Some(agent) = agent {
                command = resume_command(&agent.provider, &agent.session_id);
                // 대화를 찾은 계정 폴더로 이어 열어야 같은 대화를 찾는다. 환경 변수 앞붙임을 쓸 수 있는 셸에서만 붙인다.
                if let (Some(config_dir), "claude", true) =
                    (agent.config_dir.as_deref(), agent.provider.as_str(), is_posix_shell(&record.shell))
                {
                    command = command.map(|text| format!("CLAUDE_CONFIG_DIR={} {text}", quote_posix(config_dir)));
                }
                resumed_with = Some(agent);
            }
            RestoreTarget::Shell { shell: record.shell.clone(), wsl_distribution, cwd }
        }
    };
    let mut notice = RestoreRules::BASE_NOTICE.to_string();
    let manual = record.foreground.as_deref().and_then(|name| match name {
        "claude" => Some("claude --resume"),
        "codex" => Some("codex resume"),
        _ => None,
    });
    if let Some(agent) = resumed_with {
        notice.push_str(&format!(" Continuing the {} conversation.", display_name(&agent.provider)));
    } else if let (Some(name), Some(manual)) = (record.foreground.as_deref(), manual) {
        notice.push_str(&format!(
            " {} was running but its conversation could not be identified; run `{manual}` to choose one.",
            display_name(name)
        ));
    }
    RestorePlan { target, command, notice }
}

/// 복원된 화면 아래에 쓰는 흐린 한 줄. 화면 출력이 줄 중간에서 끝났을 수 있어 줄을 바꿔 시작한다.
pub fn notice_line(notice: &str) -> String {
    format!("\x1b[0m\r\n\x1b[2m\u{2014} {notice} \u{2014}\x1b[0m\r\n")
}

/// 새 터미널의 첫 화면: 이전 화면 꼬리, 이전 프로그램이 켠 터미널 모드 끄기, 안내 한 줄.
pub fn restored_screen(
    output: &str,
    notice: &str,
) -> String {
    format!("{output}{}{}", RestoreRules::MODE_RESET, notice_line(notice))
}

/// 프롬프트를 기다리는 한 번의 판정 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptCheck {
    /// 더 기다린다.
    Waiting,
    /// 셸이 프롬프트를 내고 입력을 받을 준비가 됐다.
    Ready,
    /// 셸 대신 다른 프로그램(tmux·fzf 등)이 앞쪽에 떠 있다. 입력하지 않는다.
    Busy,
    /// 시간 안에 조용해지지 않았다. 입력하지 않는다.
    TimedOut,
}

/// 새 셸이 프롬프트를 내고 입력을 받을 준비가 됐는지 한 번 판정한다.
///
/// 출력이 한 번이라도 나온 뒤 `PROMPT_QUIET_MS` 동안 멎고 앞쪽 프로그램이 셸일 때 `Ready`다.
/// 시작 스크립트가 tmux·fzf 같은 프로그램을 띄워 두었으면 `Busy`, 시간 안에 조용해지지 않으면 `TimedOut`이다.
/// 둘 다 이어하기 명령을 입력하지 않는다 — 입력이 엉뚱한 프로그램으로 들어가는 것보다 직접 입력하게 두는 편이 낫다.
///
/// `quiet_ms`는 마지막 출력 뒤 지난 시간(출력이 아직 없으면 None), `foreground_is_shell`은 앞쪽 프로그램이 셸인지(아직 모르면 None)이다.
///
/// Examples
/// --------
/// | 상황                                         | 결과       |
/// |----------------------------------------------|------------|
/// | 출력 뒤 0.5초 조용, 앞쪽이 셸                | `Ready`    |
/// | 출력 뒤 조용하지만 앞쪽이 tmux 등            | `Busy`     |
/// | 8초가 되도록 계속 출력                       | `TimedOut` |
/// | 출력이 아직 없음                             | `Waiting`  |
pub fn prompt_check(
    elapsed_ms: u64,
    quiet_ms: Option<u64>,
    foreground_is_shell: Option<bool>,
) -> PromptCheck {
    let is_quiet = quiet_ms.is_some_and(|quiet| quiet >= RestoreRules::PROMPT_QUIET_MS);
    match (elapsed_ms >= RestoreRules::PROMPT_TIMEOUT_MS, is_quiet, foreground_is_shell) {
        (true, _, _) => PromptCheck::TimedOut,
        (false, true, Some(true)) => PromptCheck::Ready,
        (false, true, Some(false)) => PromptCheck::Busy,
        (false, _, _) => PromptCheck::Waiting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_account::{AccountProfile, Runtime};
    use crate::agent_resume::AgentConversation;
    use crate::shell_catalog::shell_spec;

    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";

    fn record() -> HistoryRecord {
        HistoryRecord {
            version: 1,
            saved_at: 1,
            shell: "bash".into(),
            wsl_distribution: None,
            launch: None,
            cwd_start: Some("/work/app".into()),
            cwd_now: "/work/app/sub".into(),
            foreground: None,
            agent: None,
            output: "screen".into(),
        }
    }

    fn conversation(provider: &str) -> AgentConversation {
        AgentConversation { provider: provider.into(), session_id: SESSION.into(), cwd: "/w".into(), config_dir: None }
    }

    fn account_launch(provider: &str) -> Launch {
        Launch {
            program: "bash".into(),
            args: vec!["-c".into(), "x".into()],
            env: vec![("CLAUDE_CONFIG_DIR".into(), "/accounts/work".into())],
            env_remove: Vec::new(),
            cwd: Some("/work/app".into()),
            title: Some("Claude \u{b7} Work".into()),
            account: Some(AccountProfile {
                id: "a1".into(),
                provider: provider.into(),
                label: "Work".into(),
                runtime: Runtime::Native,
                wsl_distribution: None,
            }),
        }
    }

    fn shell_target(plan: &RestorePlan) -> (&str, Option<&str>, Option<&str>) {
        match &plan.target {
            RestoreTarget::Shell { shell, wsl_distribution, cwd } => {
                (shell.as_str(), wsl_distribution.as_deref(), cwd.as_deref())
            }
            RestoreTarget::Account { .. } => panic!("expected a shell terminal"),
        }
    }

    #[test]
    fn test_c_restore_1_a_shell_terminal_reopens_the_recorded_shell_in_the_current_folder() {
        let plan = plan_restore(&record());
        assert_eq!(shell_target(&plan), ("bash", None, Some("/work/app/sub")));
        assert_eq!(plan.command, None);
        assert_eq!(plan.notice, "Previous shell ended. Screen restored; commands that were running were stopped.");
    }

    #[test]
    fn test_c_restore_2_the_recorded_shell_and_distribution_are_kept_and_only_the_folder_changes() {
        let plan = plan_restore(&HistoryRecord {
            shell: "powershell".into(),
            cwd_start: Some("C:\\first".into()),
            cwd_now: "C:\\work\\now".into(),
            ..record()
        });
        assert_eq!(shell_target(&plan), ("powershell", None, Some("C:\\work\\now")));
    }

    #[test]
    fn test_c_restore_3_without_a_recorded_current_folder_the_start_folder_stays() {
        let plan = plan_restore(&HistoryRecord { cwd_now: String::new(), ..record() });
        assert_eq!(shell_target(&plan).2, Some("/work/app"));
        let nowhere = plan_restore(&HistoryRecord { cwd_now: String::new(), cwd_start: None, ..record() });
        assert_eq!(shell_target(&nowhere).2, None);
    }

    #[test]
    fn test_c_restore_4_a_claude_or_codex_conversation_becomes_a_command_typed_into_the_new_shell() {
        let claude = plan_restore(&HistoryRecord {
            agent: Some(conversation("claude")),
            foreground: Some("claude".into()),
            ..record()
        });
        assert_eq!(claude.command.as_deref(), Some(format!("claude --resume {SESSION}").as_str()));
        assert!(claude.notice.ends_with("Continuing the Claude conversation."));
        let codex = plan_restore(&HistoryRecord {
            agent: Some(conversation("codex")),
            foreground: Some("codex".into()),
            ..record()
        });
        assert_eq!(codex.command.as_deref(), Some(format!("codex resume {SESSION}").as_str()));
        assert!(codex.notice.ends_with("Continuing the Codex conversation."));
    }

    #[test]
    fn test_c_restore_5_an_agent_without_a_known_conversation_is_not_resumed_and_the_manual_command_is_told() {
        let plan = plan_restore(&HistoryRecord { foreground: Some("codex".into()), ..record() });
        assert_eq!(plan.command, None);
        assert!(plan.notice.ends_with(
            "Codex was running but its conversation could not be identified; run `codex resume` to choose one."
        ));
        let unknown = plan_restore(&HistoryRecord {
            agent: Some(conversation("gemini")),
            foreground: Some("gemini".into()),
            ..record()
        });
        assert_eq!(unknown.command, None);
        assert_eq!(unknown.notice, RestoreRules::BASE_NOTICE);
    }

    #[test]
    fn test_c_restore_6_a_claude_account_terminal_is_reopened_with_its_conversation_and_has_no_typed_command() {
        let plan = plan_restore(&HistoryRecord {
            launch: Some(account_launch("claude")),
            agent: Some(conversation("claude")),
            foreground: Some("claude".into()),
            ..record()
        });
        assert_eq!(plan.command, None);
        assert_eq!(
            plan.target,
            RestoreTarget::Account { launch: account_launch("claude"), resume_session_id: Some(SESSION.into()) }
        );
        assert!(plan.notice.ends_with("Continuing the Claude conversation."));
    }

    #[test]
    fn test_c_restore_7_a_codex_or_conversationless_account_terminal_reopens_unchanged() {
        let codex = plan_restore(&HistoryRecord {
            launch: Some(account_launch("codex")),
            agent: Some(conversation("codex")),
            foreground: Some("codex".into()),
            ..record()
        });
        assert_eq!(codex.target, RestoreTarget::Account { launch: account_launch("codex"), resume_session_id: None });
        assert_eq!(codex.command, None);
        assert!(codex.notice.contains("Codex was running but its conversation could not be identified"));
        let idle = plan_restore(&HistoryRecord { launch: Some(account_launch("claude")), ..record() });
        assert_eq!(idle.target, RestoreTarget::Account { launch: account_launch("claude"), resume_session_id: None });
        assert_eq!(idle.notice, RestoreRules::BASE_NOTICE);
    }

    #[test]
    fn test_c_restore_8_the_notice_is_a_dim_line_after_a_line_break() {
        assert_eq!(notice_line("hello"), "\x1b[0m\r\n\x1b[2m\u{2014} hello \u{2014}\x1b[0m\r\n");
        let screen = restored_screen("screen", "hello");
        assert!(
            screen.starts_with("screen\x1b[?1049l"),
            "the tail comes first and the leftover modes are switched off"
        );
        assert!(screen.ends_with(&notice_line("hello")));
    }

    #[test]
    fn test_c_restore_9_a_wsl_shell_is_reopened_with_one_start_folder_in_the_folders_distribution() {
        let plan = plan_restore(&HistoryRecord {
            shell: "wsl".into(),
            wsl_distribution: Some("Debian".into()),
            cwd_start: Some("/home/me".into()),
            cwd_now: "\\\\wsl.localhost\\Ubuntu\\home\\me\\app".into(),
            ..record()
        });
        let (shell, distribution, cwd) = shell_target(&plan);
        assert_eq!((shell, distribution, cwd), ("wsl", Some("Ubuntu"), Some("/home/me/app")));
        let spec = shell_spec(shell, cwd, distribution);
        assert_eq!(spec.args, ["-d", "Ubuntu", "--cd", "/home/me/app"], "exactly one --cd pair");
        // 복원할 폴더가 Windows 폴더이면 배포판은 기록 그대로다.
        let windows = plan_restore(&HistoryRecord {
            shell: "wsl".into(),
            wsl_distribution: Some("Debian".into()),
            cwd_now: "C:\\Users\\me".into(),
            ..record()
        });
        assert_eq!(shell_target(&windows), ("wsl", Some("Debian"), Some("C:\\Users\\me")));
    }

    #[test]
    fn test_c_restore_10_a_command_gets_the_account_folder_prefix_only_in_posix_shells() {
        let agent = AgentConversation { config_dir: Some("/acc/it's".into()), ..conversation("claude") };
        let command = |shell: &str| {
            plan_restore(&HistoryRecord {
                shell: shell.into(),
                agent: Some(agent.clone()),
                foreground: Some("claude".into()),
                ..record()
            })
            .command
        };
        let expected = format!("CLAUDE_CONFIG_DIR={} claude --resume {SESSION}", quote_posix("/acc/it's"));
        assert_eq!(command("bash").as_deref(), Some(expected.as_str()));
        assert_eq!(command("wsl").as_deref(), Some(expected.as_str()));
        assert_eq!(command("powershell").as_deref(), Some(format!("claude --resume {SESSION}").as_str()));
        assert!(is_posix_shell("/bin/zsh") && !is_posix_shell("C:\\Windows\\System32\\cmd.exe"));
    }

    #[test]
    fn test_c_restore_11_the_prompt_is_ready_only_when_output_stopped_and_a_shell_is_in_front() {
        /// 100 ms마다 판정하며 결과가 나올 때까지 시간을 흘린다.
        fn outcome(
            last_output_at: impl Fn(u64) -> Option<u64>,
            foreground_is_shell: Option<bool>,
        ) -> PromptCheck {
            let mut elapsed = 0;
            loop {
                let quiet = last_output_at(elapsed).map(|at| elapsed - at);
                let check = prompt_check(elapsed, quiet, foreground_is_shell);
                if check != PromptCheck::Waiting {
                    break check;
                }
                elapsed += 100;
            }
        }
        assert_eq!(outcome(|now| (now >= 300).then_some(300), Some(true)), PromptCheck::Ready);
        assert_eq!(outcome(|_| Some(0), Some(false)), PromptCheck::Busy);
        assert_eq!(outcome(Some, Some(true)), PromptCheck::TimedOut, "output never stops");
        assert_eq!(outcome(|_| None, Some(true)), PromptCheck::TimedOut, "no output at all");
        assert_eq!(outcome(|_| Some(0), None), PromptCheck::TimedOut, "the foreground program is never known");
    }

    #[test]
    fn test_c_d46_mode_reset_also_clears_charset_scroll_region_and_application_cursor_keys() {
        for sequence in ["\x1b(B", "\x1b7\x1b[r\x1b8", "\x1b[?1l"] {
            assert!(RestoreRules::MODE_RESET.contains(sequence), "{sequence:?} is not reset");
        }
        let screen = restored_screen("old", "notice");
        assert!(screen.starts_with("old\x1b[?1049l"), "the old screen comes first, then the reset");
        // 스크롤 영역을 푸는 `ESC [ r`은 커서를 맨 위로 보내므로 저장·복원 사이에서만 쓴다 — 안내 줄이 이전 화면을 덮어쓰지 않는다.
        let reset = RestoreRules::MODE_RESET;
        let region = reset.find("\x1b[r").expect("scroll region reset");
        assert!(reset[..region].ends_with("\x1b7") && reset[region + 3..].starts_with("\x1b8"), "{reset:?}");
    }
}
