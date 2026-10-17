//! 터미널 안 AI 에이전트(claude·codex 등)의 활동 표시와 "작업이 끝났다" 추정에 쓰는 순수 계산.
//!
//! 에이전트는 일하는 동안 화면(스피너·스트리밍 출력)을 계속 고쳐 쓰고, 끝나거나 사용자 답을 기다리면 조용해진다.
//! Enter 뒤 오래 이어진 출력이 멎으면 알린다. 작업 완료나 입력 대기를 확정하지 않는다.
//! 짧게 끝난 응답(몇 초)은 알리지 않는다 — 기다리는 사이 다른 일을 하지 않았을 가능성이 크다.
//!
//! 에이전트가 터미널 제어 시퀀스(OSC 7501, Program Status)로 "일하는 중·사용자를 기다림·끝남"을 직접 알리면
//! 그 신호를 우선한다. 출력이 오래 없는 빌드를 끝난 것으로 잘못 알리지 않고, 답을 기다리는 상태도 알 수 있다.
//! 신호를 보내지 않는 에이전트는 위의 출력 멎음 판정을 그대로 쓴다.
//!
//! 시각은 모두 호출자가 정한 기준에서 잰 밀리초(`now`)다. 이 모듈은 시계를 읽지 않는다.
use serde::{Deserialize, Serialize};
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// 활동 판정에 쓰는 시간(ms).
pub mod limit {
    /// 최근 출력과 입력 직후 대기를 구분하는 관찰 시간.
    pub const QUIET_MS: u64 = 3000;
    /// 이만큼 넘게 일한 경우만 알린다.
    pub const MIN_BUSY_MS: u64 = 5000;
    /// 완료 소리를 낸 뒤 이 시간 안에 오는 다음 소리는 내지 않는다. 여러 에이전트가 거의 동시에 끝날 때 소리가 겹쳐 울리는 것을 막는다.
    pub const SOUND_COOLDOWN_MS: u64 = 4000;
}

/// 에이전트가 터미널로 직접 알리는 상태(OSC 7501 Program Status)의 규칙.
pub mod program_status {
    pub const OSC_CODE: u32 = 7501;
}

/// 에이전트로 알아보는 프로그램 이름. 실행 파일 이름이나 스크립트 경로의 한 부분과 맞춘다.
pub const AGENT_NAMES: [&str; 11] =
    ["claude", "codex", "gemini", "opencode", "cursor-agent", "aider", "copilot", "amp", "droid", "kimi", "qwen"];

/// 스크립트를 실행하는 런타임·셸. 이 경우 실제 프로그램 이름은 스크립트 쪽에서 찾는다(`#!/bin/bash` 스크립트는 명령줄이 `/bin/bash 스크립트`다).
/// 스크립트 없이 셸만 떠 있으면 셸 이름이다.
const RUNTIMES: [&str; 8] = ["node", "bun", "deno", "python", "python3", "bash", "sh", "zsh"];

/// 대화형 셸 이름. 앞쪽 프로그램이 이 이름이면 터미널은 명령을 기다리는 빈 셸이다.
const SHELL_NAMES: [&str; 12] =
    ["bash", "sh", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "pwsh", "powershell", "cmd"];

/// 에이전트가 직접 알린 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusState {
    Working,
    Blocked,
    Done,
    Idle,
    Error,
    /// 에이전트가 상태 표시를 거둔다는 뜻. 저장되지 않고 신호를 지운다.
    Clear,
}

impl StatusState {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "working" => Some(Self::Working),
            "blocked" => Some(Self::Blocked),
            "done" => Some(Self::Done),
            "idle" => Some(Self::Idle),
            "error" => Some(Self::Error),
            "clear" => Some(Self::Clear),
            _ => None,
        }
    }
}

/// OSC 7501 본문의 해석 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramStatus {
    /// 지원 여부를 묻는 질의(`?`). 터미널이 같은 시퀀스로 답해야 에이전트가 알림을 보내기 시작한다.
    Query,
    /// 상태 알림. `kind`는 `blocked`의 이유 같은 보조 값이다.
    Report { state: StatusState, kind: Option<String> },
}

/// 에이전트가 알린 상태의 기록.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    pub state: StatusState,
    pub kind: Option<String>,
    /// 지금 이어지는 `working`이 시작된 시각. 한 번도 일하지 않았으면 None.
    pub working_since: Option<u64>,
    /// 다음 `settle`이 한 번 알려야 한다.
    pub alert: bool,
}

/// 터미널 하나의 에이전트 활동 기록.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// 마지막 Enter 시각. 그 뒤의 출력만 "일"로 센다.
    pub armed_at: Option<u64>,
    /// Enter 뒤 첫 출력 시각.
    pub busy_since: Option<u64>,
    pub last_output: Option<u64>,
    /// 일하는 동안 한 번이라도 에이전트였다.
    pub agent: bool,
    pub signal: Option<Signal>,
}

impl Activity {
    /// 아직 일하지 않는 상태.
    pub fn idle() -> Self {
        Self::default()
    }
}

/// 화면에 보이는 활동 상태. 내부 이름이고 화면 글자는 Sent(waiting)·Output(working)·Quiet(quiet)이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityState {
    /// Enter를 보낸 지 3초가 안 됐고 아직 출력이 없다.
    Waiting,
    /// 최근 3초 안에 출력이 있었다.
    Working,
    Quiet,
    /// 앞쪽 프로그램을 아직 모른다.
    Unknown,
}

/// `settle`의 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    pub activity: Activity,
    /// 참이면 지금 알릴 때다.
    pub finished: bool,
}

fn base_name(file: &str) -> &str {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let lower = name.to_ascii_lowercase();
    let cut = [".exe", ".cmd", ".js", ".mjs", ".cjs", ".py"].iter().find(|suffix| lower.ends_with(**suffix));
    match cut {
        Some(suffix) => &name[..name.len() - suffix.len()],
        None => name,
    }
}

/// 인자가 `-i`를 품은 짧은 옵션 묶음(`-i`, `-il`)인지.
fn is_interactive_flag(arg: &str) -> bool {
    arg.strip_prefix('-').is_some_and(|rest| rest.split('-').next().is_some_and(|group| group.contains('i')))
}

/// 명령줄(argv)에서 사람이 알아볼 프로그램 이름을 뽑는다. 에이전트면 에이전트 이름이다.
/// 빈 명령줄이면 None.
///
/// Examples
/// --------
/// | argv                                                           | 결과     |
/// |----------------------------------------------------------------|----------|
/// | `/home/me/.local/bin/claude`                                   | `claude` |
/// | `node /usr/lib/node_modules/@openai/codex/bin/codex.js`        | `codex`  |
/// | `node server.js`                                               | `server` |
/// | `-bash`                                                        | `bash`   |
pub fn program_name<S: AsRef<str>>(argv: &[S]) -> Option<String> {
    let first = argv.first().map(AsRef::as_ref).filter(|first| !first.is_empty())?;
    let executable = base_name(first);
    let executable = executable.strip_prefix('-').unwrap_or(executable);
    // 대화형 bash는 --rcfile로 시작 파일 이름을 받을 수 있다. 그 파일은 실행 중인 프로그램이 아니다.
    let interactive_bash = executable == "bash" && argv[1..].iter().any(|arg| is_interactive_flag(arg.as_ref()));
    let script = if RUNTIMES.contains(&executable) && !interactive_bash {
        argv[1..].iter().map(AsRef::as_ref).find(|arg| !arg.is_empty() && !arg.starts_with('-'))
    } else {
        None
    };
    let look_in: Vec<String> = std::iter::once(executable)
        .chain(script.into_iter().flat_map(|script| script.split(['/', '\\'])))
        .map(str::to_lowercase)
        .collect();
    let agent = AGENT_NAMES.iter().find(|agent| {
        look_in.iter().any(|part| {
            let bare = ["js", "mjs", "cjs"]
                .iter()
                .find_map(|extension| part.strip_suffix(&format!(".{extension}")))
                .unwrap_or(part);
            part == *agent || bare == **agent || *part == format!("{agent}-code")
        })
    });
    let name = match (agent, script) {
        (Some(agent), _) => (*agent).to_string(),
        (None, Some(script)) => base_name(script).to_string(),
        (None, None) => executable.to_string(),
    };
    Some(name)
}

/// 프로그램 이름이 에이전트인지.
pub fn is_agent(name: &str) -> bool {
    AGENT_NAMES.contains(&name)
}

/// 프로그램 이름(`program_name`의 결과)이 대화형 셸인지. 셸만 떠 있는 터미널은 셸을 바꿔도 잃을 실행 중인 작업이 없다.
/// 셸이 실행한 스크립트는 스크립트 이름이라 셸이 아니고, 이름을 모르면(None) 셸로 보지 않는다.
///
/// Examples
/// --------
/// | 이름           | 결과    |
/// |----------------|---------|
/// | `bash`, `pwsh` | `true`  |
/// | `build`, `claude`, 모름 | `false` |
pub fn is_shell(name: Option<&str>) -> bool {
    name.is_some_and(|name| SHELL_NAMES.contains(&name))
}

/// 사용자 입력. Enter(`\r`)가 들어오면 그 뒤의 출력을 "일"로 센다.
///
/// 에이전트가 알려 온 상태(signal)는 입력으로 지우지 않는다. 이 터미널이 신호를 보낸다는 사실이 이어져야 한다.
pub fn note_input(
    activity: &Activity,
    data: &[u8],
    now: u64,
) -> Activity {
    if data.contains(&b'\r') {
        Activity { armed_at: Some(now), signal: activity.signal.clone(), ..Activity::idle() }
    } else {
        activity.clone()
    }
}

/// 프로그램 출력을 최근 활동으로 기록한다. 완료 알림의 작업 시간은 Enter 뒤의 출력만 센다.
/// 시작 화면·프롬프트 다시 그리기는 완료 알림으로 세지 않지만, 출력이 재개됐다는 표시는 유지한다.
pub fn note_output(
    activity: &Activity,
    now: u64,
) -> Activity {
    let busy_since = if activity.armed_at.is_none() { activity.busy_since } else { activity.busy_since.or(Some(now)) };
    Activity { busy_since, last_output: Some(now), ..activity.clone() }
}

/// 에이전트가 OSC 7501로 보낸 본문을 해석한다. 지원 여부를 묻는 질의(`?`)이거나 상태 알림이다.
/// 서브에이전트의 기록(`id=…`이 붙음)과 알 수 없는 상태는 터미널 전체의 상태가 아니므로 None이다.
///
/// Examples
/// --------
/// | 본문                                              | 결과                                   |
/// |---------------------------------------------------|----------------------------------------|
/// | `?`                                               | `Query`                                |
/// | `state=working:app=claude-code`                   | `Report { Working, None }`             |
/// | `state=blocked:app=claude-code:kind=permission`   | `Report { Blocked, Some("permission") }` |
/// | `state=working:app=claude-code:id=sub1`           | `None`                                 |
/// | `state=sleeping`                                  | `None`                                 |
pub fn parse_program_status(data: &str) -> Option<ProgramStatus> {
    if data.starts_with('?') {
        Some(ProgramStatus::Query)
    } else {
        let mut fields: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        for field in data.split(':') {
            match field.find('=') {
                Some(at) if at > 0 => fields.insert(&field[..at], &field[at + 1..]),
                _ => fields.insert(field, ""),
            };
        }
        let state = fields.get("state").and_then(|name| StatusState::parse(name));
        match state {
            Some(state) if !fields.contains_key("id") => {
                Some(ProgramStatus::Report { state, kind: fields.get("kind").map(|kind| kind.to_string()) })
            }
            _ => None,
        }
    }
}

/// 에이전트가 알린 상태를 활동 기록에 넣는다. 알릴 때(`alert`)가 되면 다음 `settle`이 한 번 알린다.
///
/// 알리는 경우: 사용자를 기다리게 되었을 때(blocked)·오류가 났을 때(error)는 바로, 끝났을 때(done)는 `MIN_BUSY_MS` 넘게
/// 일한 뒤에만 알린다. 사용자를 기다리는 에이전트는 시간과 무관하게 사용자가 필요해서다.
/// `clear`는 에이전트가 상태 표시를 거둔다는 뜻이라 신호 사용을 멈추고 출력 멎음 판정으로 돌아간다.
/// 질의(`Query`)는 기록을 바꾸지 않는다.
///
/// Examples
/// --------
/// | 흐름                              | 알림           |
/// |-----------------------------------|----------------|
/// | working(0) → done(6000)           | 알림           |
/// | working(0) → done(2000)           | 없음           |
/// | working(0) → blocked(1000)        | 알림           |
/// | done → blocked → blocked          | 첫 blocked만   |
pub fn note_status(
    activity: &Activity,
    status: &ProgramStatus,
    now: u64,
) -> Activity {
    match status {
        ProgramStatus::Query => activity.clone(),
        ProgramStatus::Report { state: StatusState::Clear, .. } => Activity { signal: None, ..activity.clone() },
        ProgramStatus::Report { state, kind } => {
            let previous = activity.signal.as_ref();
            let is_working = previous.is_some_and(|previous| previous.state == StatusState::Working);
            let previous_since = previous.and_then(|previous| previous.working_since);
            let working_since = if *state == StatusState::Working && !is_working { Some(now) } else { previous_since };
            let mut alert = previous.is_some_and(|previous| previous.alert);
            match state {
                StatusState::Blocked | StatusState::Error => {
                    alert = alert || previous.map(|previous| previous.state) != Some(*state);
                }
                StatusState::Done => {
                    alert = alert
                        || (is_working
                            && previous_since.is_some_and(|since| now.saturating_sub(since) >= limit::MIN_BUSY_MS));
                }
                _ => {}
            }
            Activity {
                signal: Some(Signal { state: *state, kind: kind.clone(), working_since, alert }),
                ..activity.clone()
            }
        }
    }
}

/// 완료 소리를 낼지. 마지막 소리 뒤 `SOUND_COOLDOWN_MS`가 지났거나 아직 낸 적이 없을 때만 낸다.
/// 소리만 거르고 완료 표시(미확인 점)는 그대로 붙는다 — 어느 터미널이 끝났는지는 표시로 알 수 있다.
///
/// Examples
/// --------
/// | last_sound_at | now  | 결과    |
/// |---------------|------|---------|
/// | None          | 1000 | `true`  |
/// | 1000          | 3000 | `false` |
/// | 1000          | 5000 | `true`  |
pub fn should_sound(
    last_sound_at: Option<u64>,
    now: u64,
) -> bool {
    last_sound_at.is_none_or(|last| now.saturating_sub(last) >= limit::SOUND_COOLDOWN_MS)
}

/// 터미널 입력·출력으로 관찰한 에이전트 상태. 프로그램을 모르면(`agent_now`가 None) `Unknown`, 에이전트가 아니면 None이다.
///
/// Enter 직후 첫 출력을 기다리는 3초는 `Waiting`, 최근 출력은 `Working`, 그 밖에는 `Quiet`이다.
/// 초기 화면이나 프롬프트를 다시 그리는 출력도 최근 활동에 포함된다.
/// 출력이 멎어도 내부 계산이나 사용자 답을 기다리는지는 알 수 없으므로 완료 여부는 확정하지 않는다.
/// 완료 알림의 최소 작업 시간과 달리 짧은 응답도 같은 표시 규칙을 쓴다.
pub fn activity_state(
    activity: &Activity,
    now: u64,
    agent_now: Option<bool>,
) -> Option<ActivityState> {
    match agent_now {
        None => Some(ActivityState::Unknown),
        Some(false) => None,
        Some(true) => {
            let recent = |time: u64| now.saturating_sub(time) < limit::QUIET_MS;
            let state = if activity.signal.as_ref().is_some_and(|signal| signal.state == StatusState::Working) {
                // 에이전트가 일하는 중이라고 직접 알렸다. 출력이 없어도 조용한 것이 아니다.
                ActivityState::Working
            } else if activity.busy_since.is_none() && activity.armed_at.is_some_and(recent) {
                ActivityState::Waiting
            } else if activity.last_output.is_some_and(recent) {
                ActivityState::Working
            } else {
                ActivityState::Quiet
            };
            Some(state)
        }
    }
}

/// 주기적으로 불러 긴 출력이 멎었는지 본다. `agent_now`는 지금 이 터미널의 프로그램이 에이전트인지이고, 모르면 None이다.
/// 일하는 동안 한 번이라도 에이전트였으면 끝났을 때 알린다(`claude -p`처럼 끝나며 셸로 돌아가는 경우 포함).
/// 에이전트가 상태를 직접 알려 온 터미널(`signal`)은 출력 멎음을 보지 않고 그 신호가 알릴 때를 정한다.
pub fn settle(
    activity: &Activity,
    now: u64,
    agent_now: Option<bool>,
) -> Settled {
    let mut next = if activity.busy_since.is_some() && agent_now == Some(true) {
        Activity { agent: true, ..activity.clone() }
    } else {
        activity.clone()
    };
    let mut finished = false;
    if let Some(signal) = next.signal.clone() {
        // 에이전트가 직접 알린 상태가 있으면 출력이 멎은 것은 보지 않는다. 출력 없이 도는 빌드는 끝난 것이 아니다.
        if agent_now == Some(false) {
            // 에이전트가 끝나 셸로 돌아왔다. 알릴 것이 남았거나 일하다 끊겼다면 알리고 기록을 비운다.
            let was_working = signal.state == StatusState::Working
                && signal.working_since.is_some_and(|since| now.saturating_sub(since) >= limit::MIN_BUSY_MS);
            finished = signal.alert || was_working;
            next = Activity::idle();
        } else if signal.alert {
            finished = true;
            next.signal = Some(Signal { alert: false, ..signal });
        }
    } else if let (Some(busy_since), Some(last_output)) = (next.busy_since, next.last_output)
        && now.saturating_sub(last_output) >= limit::QUIET_MS
    {
        finished = next.agent && last_output.saturating_sub(busy_since) >= limit::MIN_BUSY_MS;
        next = Activity::idle();
    }
    Settled { activity: next, finished }
}

/// 터미널 제목에서 에이전트가 진행 표시로 붙인 맨 앞 기호를 뗀 것.
///
/// Claude Code 같은 에이전트는 일하는 동안 제목 맨 앞 기호를 1초쯤마다 바꾼다(`◐ 작업` ↔ `◑ 작업`). 화면 목록을 다시 그릴지 정할 때
/// 이 값을 비교하면, 기호만 바뀐 제목 변경으로 사이드바·탭 줄을 다시 그려 키 입력 처리를 늦추지 않는다.
/// 맨 앞이 기호 문자(유니코드 So) 하나이고 그 뒤가 공백이거나 끝일 때만 뗀다. 글자·숫자·경로로 시작하는 제목은 그대로다.
///
/// Examples
/// --------
/// | 제목                   | 결과               |
/// |------------------------|--------------------|
/// | `◐ Rivers paragraph`   | `Rivers paragraph` |
/// | `✳ Claude Code`        | `Claude Code`      |
/// | `⠂ build`              | `build`            |
/// | `◐`                    | (빈 글자)          |
/// | `~/work`               | `~/work`           |
pub fn title_without_progress(title: &str) -> &str {
    let mut chars = title.chars();
    match chars.next() {
        Some(first) if first.general_category() == GeneralCategory::OtherSymbol => {
            let rest = chars.as_str();
            if rest.is_empty() {
                rest
            } else if rest.starts_with(char::is_whitespace) {
                rest.trim_start()
            } else {
                title
            }
        }
        _ => title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limit::{MIN_BUSY_MS, QUIET_MS, SOUND_COOLDOWN_MS};

    /// 200 ms 간격으로 `from`부터 `to`(포함)까지 출력을 기록한다.
    fn output_every_200ms(
        mut activity: Activity,
        from: u64,
        to: u64,
    ) -> Activity {
        let mut at = from;
        while at <= to {
            activity = note_output(&activity, at);
            at += 200;
        }
        activity
    }

    fn working() -> ProgramStatus {
        ProgramStatus::Report { state: StatusState::Working, kind: None }
    }

    fn report(state: StatusState) -> ProgramStatus {
        ProgramStatus::Report { state, kind: None }
    }

    #[test]
    fn test_c_agent_a1_1_program_name_comes_from_the_command_line() {
        assert_eq!(program_name(&["/home/me/.local/bin/claude", "--resume"]).as_deref(), Some("claude"));
        assert_eq!(
            program_name(&["node", "/usr/lib/node_modules/@openai/codex/bin/codex.js"]).as_deref(),
            Some("codex")
        );
        assert_eq!(
            program_name(&["node", "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js"]).as_deref(),
            Some("claude")
        );
        assert_eq!(program_name(&["C:\\Tools\\codex.exe"]).as_deref(), Some("codex"));
        assert_eq!(program_name(&["node", "server.js"]).as_deref(), Some("server"));
        assert_eq!(program_name(&["-bash"]).as_deref(), Some("bash"));
        assert_eq!(program_name(&["/bin/bash", "/home/me/bin/claude"]).as_deref(), Some("claude"));
        assert_eq!(program_name(&["/bin/bash", "--rcfile", "/dev/fd/56", "-i"]).as_deref(), Some("bash"));
        assert_eq!(program_name(&["/bin/zsh", "-l"]).as_deref(), Some("zsh"));
        assert_eq!(program_name::<&str>(&[]), None);
        assert!(is_agent("claude"));
        assert!(!is_agent("bash"));
    }

    #[test]
    fn test_c_account_idle_judge_only_shell_names_are_an_empty_shell() {
        let shells: [&[&str]; 6] = [
            &["-bash"],
            &["/bin/zsh", "-l"],
            &["/usr/bin/fish"],
            &["C:\\Program Files\\PowerShell\\7\\pwsh.exe"],
            &["powershell.exe", "-NoLogo"],
            &["cmd.exe"],
        ];
        for argv in shells {
            assert!(is_shell(program_name(argv).as_deref()), "{argv:?}");
        }
        let not_shells: [&[&str]; 4] = [
            &["/bin/bash", "/home/me/build.sh"],
            &["/home/me/.local/bin/claude"],
            &["node", "server.js"],
            &["vim", "a.txt"],
        ];
        for argv in not_shells {
            assert!(!is_shell(program_name(argv).as_deref()), "{argv:?}");
        }
        assert!(!is_shell(None));
    }

    #[test]
    fn test_c_agent_a2_1_a_long_job_that_goes_quiet_is_announced_once() {
        let mut activity = note_input(&Activity::idle(), b"fix the bug\r", 0);
        activity = output_every_200ms(activity, 100, MIN_BUSY_MS + 1000);
        let last = MIN_BUSY_MS + 1000;
        let mut settled = settle(&activity, last + 500, Some(true));
        assert!(!settled.finished, "the quiet time is still short");
        settled = settle(&settled.activity, last + QUIET_MS, Some(true));
        assert!(settled.finished);
        assert!(!settle(&settled.activity, last + QUIET_MS * 3, Some(true)).finished, "announced once");
    }

    #[test]
    fn test_c_agent_a2_2_short_replies_output_without_enter_and_non_agents_are_not_announced() {
        let mut activity = note_output(&note_input(&Activity::idle(), b"\r", 0), 1000);
        assert!(!settle(&activity, 1000 + QUIET_MS, Some(true)).finished, "a short reply");
        activity = output_every_200ms(Activity::idle(), 0, 10000);
        assert!(!settle(&activity, 10000 + QUIET_MS, Some(true)).finished, "output without Enter (start screen)");
        activity = output_every_200ms(note_input(&Activity::idle(), b"npm test\r", 0), 100, 10000);
        assert!(!settle(&activity, 10000 + QUIET_MS, Some(false)).finished, "a long command that is not an agent");
    }

    #[test]
    fn test_c_agent_a2_3_an_agent_that_returns_to_the_shell_is_still_announced() {
        let mut activity = note_input(&Activity::idle(), b"claude -p \"summarize\"\r", 0);
        activity = output_every_200ms(activity, 100, 8000);
        activity = settle(&activity, 8100, Some(true)).activity;
        assert!(settle(&activity, 8000 + QUIET_MS, Some(false)).finished);
    }

    #[test]
    fn test_c_agent_a3_1_input_wait_output_and_quiet_are_told_apart() {
        let initial = Activity::idle();
        assert_eq!(activity_state(&initial, 0, Some(true)), Some(ActivityState::Quiet));
        assert_eq!(activity_state(&note_input(&initial, b"typing", 100), 100, Some(true)), Some(ActivityState::Quiet));
        let submitted = note_input(&initial, b"run\r", 200);
        assert_eq!(activity_state(&submitted, 300, Some(true)), Some(ActivityState::Waiting));
        let output = note_output(&submitted, 400);
        assert_eq!(activity_state(&output, 400 + QUIET_MS - 1, Some(true)), Some(ActivityState::Working));
        assert_eq!(activity_state(&output, 400 + QUIET_MS, Some(true)), Some(ActivityState::Quiet));
        let settled = settle(&output, 400 + QUIET_MS, Some(true)).activity;
        assert_eq!(activity_state(&settled, 400 + QUIET_MS, Some(true)), Some(ActivityState::Quiet));
    }

    #[test]
    fn test_c_agent_a3_2_no_state_is_shown_once_the_agent_left_for_the_shell() {
        let output = note_output(&note_input(&Activity::idle(), b"\r", 0), 100);
        assert_eq!(activity_state(&output, 200, Some(false)), None);
        assert_eq!(activity_state(&Activity::idle(), 200, Some(false)), None);
    }

    #[test]
    fn test_c_agent_a3_3_output_resuming_without_enter_brings_back_working() {
        let output = note_output(&note_input(&Activity::idle(), b"\r", 0), 100);
        let paused = settle(&output, 100 + QUIET_MS, Some(true)).activity;
        assert_eq!(activity_state(&paused, 100 + QUIET_MS, Some(true)), Some(ActivityState::Quiet));
        let resumed = note_output(&paused, 5000);
        assert_eq!(activity_state(&resumed, 5000, Some(true)), Some(ActivityState::Working));
        assert!(
            !settle(&resumed, 5000 + QUIET_MS, Some(true)).finished,
            "output without input never repeats the announcement"
        );
        assert_eq!(
            activity_state(&note_output(&Activity::idle(), 0), 1, Some(true)),
            Some(ActivityState::Working),
            "start-screen output also counts as recent activity"
        );
    }

    #[test]
    fn test_c_agent_a3_4_sent_ends_at_the_three_second_edge_and_becomes_quiet() {
        let sent = note_input(&Activity::idle(), b"\r", 100);
        assert_eq!(activity_state(&sent, 100 + QUIET_MS - 1, Some(true)), Some(ActivityState::Waiting));
        assert_eq!(activity_state(&sent, 100 + QUIET_MS, Some(true)), Some(ActivityState::Quiet));
        assert_eq!(activity_state(&sent, 60000, Some(true)), Some(ActivityState::Quiet));
        let late_output = note_output(&sent, 60001);
        assert_eq!(activity_state(&late_output, 60001, Some(true)), Some(ActivityState::Working));
    }

    #[test]
    fn test_c_agent_a3_5_every_history_maps_to_exactly_one_state() {
        use ActivityState::{Quiet, Waiting, Working};
        let histories: [(Activity, [ActivityState; 4]); 4] = [
            (Activity::idle(), [Quiet, Quiet, Quiet, Quiet]),
            (note_input(&Activity::idle(), b"\r", 100), [Waiting, Quiet, Quiet, Quiet]),
            (note_output(&Activity::idle(), 200), [Working, Working, Quiet, Quiet]),
            (note_output(&note_input(&Activity::idle(), b"\r", 100), 200), [Working, Working, Quiet, Quiet]),
        ];
        for (history, expected_states) in histories {
            for (index, now) in [200, 3100, 3200, 10000].into_iter().enumerate() {
                assert_eq!(activity_state(&history, now, Some(true)), Some(expected_states[index]));
                assert_eq!(activity_state(&history, now, Some(false)), None);
                assert_eq!(activity_state(&history, now, None), Some(ActivityState::Unknown));
            }
        }
    }

    #[test]
    fn test_c_agent_a4_1_osc_7501_bodies_are_parsed_and_sub_agents_and_unknown_states_dropped() {
        assert_eq!(parse_program_status("?"), Some(ProgramStatus::Query));
        assert_eq!(parse_program_status("state=working:app=claude-code"), Some(working()));
        assert_eq!(
            parse_program_status("state=blocked:app=claude-code:kind=permission"),
            Some(ProgramStatus::Report { state: StatusState::Blocked, kind: Some("permission".to_string()) })
        );
        assert_eq!(parse_program_status("state=done:app=claude-code:msg=aGVsbG8="), Some(report(StatusState::Done)));
        assert_eq!(parse_program_status("state=working:app=claude-code:id=sub1"), None);
        assert_eq!(parse_program_status("state=sleeping"), None);
        assert_eq!(parse_program_status("garbage"), None);
    }

    #[test]
    fn test_c_agent_a4_2_a_silent_long_build_is_not_announced_until_done_arrives() {
        let mut activity = note_input(&Activity::idle(), b"run the tests\r", 0);
        activity = note_status(&activity, &working(), 100);
        activity = note_output(&activity, 200);
        // 마지막 출력 뒤 25초 동안 출력이 없다. 출력 멎음 판정이었다면 이미 알렸을 시간이다.
        let mut settled = settle(&activity, 200 + 25000, Some(true));
        assert!(!settled.finished);
        assert_eq!(
            activity_state(&activity, 200 + 25000, Some(true)),
            Some(ActivityState::Working),
            "quiet but still shown as working"
        );
        activity = note_status(&settled.activity, &report(StatusState::Done), 200 + 25000);
        settled = settle(&activity, 200 + 25500, Some(true));
        assert!(settled.finished);
        assert!(!settle(&settled.activity, 200 + 26000, Some(true)).finished, "announced once");
        const { assert!(25000 > MIN_BUSY_MS) };
    }

    #[test]
    fn test_c_agent_a4_3_waiting_for_the_user_is_announced_at_once_and_short_replies_are_not() {
        let blocked = ProgramStatus::Report { state: StatusState::Blocked, kind: Some("permission".to_string()) };
        let mut activity = note_status(&Activity::idle(), &working(), 0);
        let mut settled = settle(&note_status(&activity, &blocked, 1000), 1100, Some(true));
        assert!(settled.finished, "a blocked agent needs the user whatever the elapsed time");
        let again = settle(&note_status(&settled.activity, &blocked, 1500), 1600, Some(true));
        assert!(!again.finished, "the same wait is not announced twice");
        activity = note_status(&Activity::idle(), &working(), 0);
        settled = settle(&note_status(&activity, &report(StatusState::Done), 2000), 2100, Some(true));
        assert!(!settled.finished, "a reply of a few seconds is not announced");
    }

    #[test]
    fn test_c_agent_a4_4_input_keeps_the_signal_and_clear_returns_to_quiet_detection() {
        let mut activity = note_status(&Activity::idle(), &report(StatusState::Idle), 0);
        activity = note_input(&activity, b"go\r", 10);
        assert_eq!(activity.signal.as_ref().map(|signal| signal.state), Some(StatusState::Idle));
        activity = note_status(&activity, &report(StatusState::Clear), 20);
        assert!(activity.signal.is_none());
        activity = output_every_200ms(activity, 100, MIN_BUSY_MS + 1000);
        assert!(settle(&activity, MIN_BUSY_MS + 1000 + QUIET_MS, Some(true)).finished);
    }

    #[test]
    fn test_c_agent_a4_5_an_agent_that_left_for_the_shell_is_announced_only_if_cut_off_while_working() {
        let mut activity = note_status(&Activity::idle(), &working(), 0);
        let settled = settle(&activity, MIN_BUSY_MS + 1, Some(false));
        assert!(settled.finished);
        assert!(settled.activity.signal.is_none());
        activity = note_status(&Activity::idle(), &report(StatusState::Idle), 0);
        assert!(!settle(&activity, 60000, Some(false)).finished);
        let unknown = settle(&note_status(&Activity::idle(), &working(), 0), 1000, None);
        assert!(!unknown.finished, "an unknown program defers the decision");
    }

    #[test]
    fn test_am_sound_a_second_done_sound_inside_the_cooldown_is_skipped() {
        assert!(should_sound(None, 1000));
        assert!(!should_sound(Some(1000), 1000 + SOUND_COOLDOWN_MS - 1));
        assert!(should_sound(Some(1000), 1000 + SOUND_COOLDOWN_MS));
    }

    #[test]
    fn test_am_title_progress_symbols_alone_do_not_change_the_title() {
        assert_eq!(title_without_progress("◐ Rivers paragraph"), title_without_progress("◑ Rivers paragraph"));
        assert_eq!(title_without_progress("✳ Claude Code"), "Claude Code");
        assert_eq!(title_without_progress("⠂ build"), "build");
        assert_eq!(title_without_progress("◐"), title_without_progress("◑"));
        assert_eq!(title_without_progress("◐x"), "◐x");
        assert_eq!(title_without_progress("~/work"), "~/work");
        assert_eq!(title_without_progress("bash"), "bash");
        assert_eq!(title_without_progress(""), "");
    }
}
