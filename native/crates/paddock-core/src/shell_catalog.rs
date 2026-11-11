//! 셸 이름 → 실행 파일·인자·환경 변수. 터미널을 띄울 때 데몬이 쓰는 순수 규칙이다.
//! 터미널의 실행 환경 이름(셸·WSL 배포판)을 정하는 규칙도 여기 둔다.
use std::collections::BTreeMap;

use crate::agent_account::{AccountProfile, Runtime};
use crate::cwd_report::{cwd_report_options, percent_decode};
use crate::wsl_probe::is_wsl_shell;

/// 셸 프로세스를 띄우는 데 필요한 값.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: String,
    pub args: Vec<String>,
    /// 셸을 시작할 폴더. 셸 해석을 거치지 않고 프로세스 작업 폴더로 넘긴다.
    pub working_directory: Option<String>,
    pub env: Vec<(String, String)>,
}

/// 셸 이름 하나를 실행 명세로 바꾼다.
///
/// Windows 셸(PowerShell·명령 프롬프트·Git Bash)은 프롬프트마다 현재 폴더를 알리도록 인자·환경 변수를 덧붙인다
/// (`cwd_report` 참고). 데몬이 폴더를 알 방법이 그것뿐이기 때문이다.
///
/// 폴더(`cwd`)는 인자 문자열에 끼워 넣지 않는다 — 공백·따옴표·`$(…)`가 든 경로도 한 덩어리로 전달된다.
/// `wsl`만 인자(`--cd <cwd>`)로 받아야 하는데, 그때도 경로는 인자 배열의 한 원소다. 셸 문자열을 거치지 않는다.
/// 이름이 목록에 없으면 그 이름을 실행 파일로 그대로 쓴다.
///
/// Examples
/// --------
/// | name       | cwd        | wsl_distribution | program          | args                            |
/// |------------|------------|------------------|------------------|---------------------------------|
/// | powershell | `C:\work`  | -                | `powershell.exe` | `-NoLogo` (작업 폴더 `C:\work`) |
/// | wsl        | `/home/a`  | `Debian`         | `wsl.exe`        | `-d Debian --cd /home/a`        |
/// | fish       | -          | -                | `fish`           | (없음)                          |
pub fn shell_spec(
    name: &str,
    cwd: Option<&str>,
    wsl_distribution: Option<&str>,
) -> ShellSpec {
    let (program, mut args): (&str, Vec<String>) = match name {
        "powershell" => ("powershell.exe", vec!["-NoLogo".to_string()]),
        "cmd" => ("cmd.exe", Vec::new()),
        "wsl" => ("wsl.exe", Vec::new()),
        "gitbash" => (r"C:\Program Files\Git\bin\bash.exe", vec!["--login".to_string(), "-i".to_string()]),
        "bash" => ("bash", vec!["-i".to_string()]),
        other => (other, Vec::new()),
    };
    let mut working_directory = cwd.map(str::to_string);
    if name == "wsl" {
        if let Some(distribution) = wsl_distribution {
            args.push("-d".to_string());
            args.push(distribution.to_string());
        }
        if let Some(dir) = working_directory.take() {
            args.push("--cd".to_string());
            args.push(dir);
        }
    }
    let mut env =
        vec![("TERM".to_string(), "xterm-256color".to_string()), ("COLORTERM".to_string(), "truecolor".to_string())];
    if let Some(report) = cwd_report_options(program, &args) {
        args = report.shell_args;
        env.extend(report.env);
    }
    ShellSpec { program: program.to_string(), args, working_directory, env }
}

/// 설정이 없을 때 새 터미널이 쓰는 셸 이름. Windows는 기본 WSL 배포판이 있으면 `wsl`, 없으면 `powershell`이고 그 밖의 OS는 `bash`다.
///
/// Examples
/// --------
/// | is_windows | has_wsl_distribution | 결과         |
/// |------------|----------------------|--------------|
/// | true       | true                 | `wsl`        |
/// | true       | false                | `powershell` |
/// | false      | (무관)               | `bash`       |
pub fn default_shell_name(
    is_windows: bool,
    has_wsl_distribution: bool,
) -> &'static str {
    match (is_windows, has_wsl_distribution) {
        (true, true) => "wsl",
        (true, false) => "powershell",
        (false, _) => "bash",
    }
}

/// 터미널을 연 셸 설정 중 실행 환경 이름에 쓰는 부분.
#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalOptions<'a> {
    pub shell_path: &'a str,
    pub shell_args: &'a [String],
    /// 계정 버튼으로 연 터미널의 계정.
    pub account: Option<&'a AccountProfile>,
}

/// 터미널이 쓸 WSL 배포판 이름. 작업 폴더의 배포판이 터미널을 연 설정의 기본 배포판보다 우선한다. 모르면 빈 글자다.
///
/// Examples
/// --------
/// | cwd                                       | 인자          | 결과        |
/// |-------------------------------------------|---------------|-------------|
/// | `file://wsl.localhost/Debian/home/me`     | `-d Ubuntu`   | `Debian`    |
/// | `file://wsl$/Ubuntu%2024/home/me`         | (없음)        | `Ubuntu 24` |
/// | (없음)                                    | `-d Ubuntu`   | `Ubuntu`    |
pub fn distribution_of(
    options: &TerminalOptions,
    cwd: &str,
) -> String {
    let lower = cwd.to_ascii_lowercase();
    let from_folder = ["file://wsl.localhost", "file://wsl$", "\\\\wsl.localhost", "\\\\wsl$"]
        .iter()
        .find(|prefix| lower.starts_with(**prefix))
        .and_then(|prefix| {
            let rest = &cwd[prefix.len()..];
            let rest = rest.strip_prefix(['\\', '/'])?;
            rest.split(['\\', '/']).next().filter(|name| !name.is_empty()).map(percent_decode)
        });
    let from_account =
        options.account.and_then(|account| account.wsl_distribution.clone()).filter(|name| !name.is_empty());
    let args = options.shell_args;
    let from_flag = args
        .iter()
        .position(|argument| argument == "-d" || argument == "--distribution")
        .and_then(|index| args.get(index + 1).cloned());
    let from_equals = args.iter().find_map(|argument| argument.strip_prefix("--distribution=").map(str::to_string));
    from_folder
        .or(from_account)
        .or(from_flag.filter(|name| !name.is_empty()))
        .or(from_equals.filter(|name| !name.is_empty()))
        .unwrap_or_default()
}

/// WSL 공유 경로(`\\wsl.localhost\배포판\…`)를 배포판 이름과 그 안의 Linux 경로로 나눈다. 그 밖의 경로는 None이다.
///
/// 작업 폴더의 key가 이 모양으로 오므로, 그 폴더에 새 터미널을 열 때 `wsl.exe -d 배포판 --cd 경로`를 만드는 데 쓴다.
///
/// Examples
/// --------
/// | 경로                                      | 결과                          |
/// |-------------------------------------------|-------------------------------|
/// | `\\wsl.localhost\Ubuntu\home\me\app`    | `Some(("Ubuntu", "/home/me/app"))` |
/// | `\\wsl$\Debian`                          | `Some(("Debian", "/"))`       |
/// | `C:/Users/me`                             | None                          |
pub fn wsl_folder(path: &str) -> Option<(String, String)> {
    let unified = path.replace('\\', "/");
    let lower = unified.to_ascii_lowercase();
    let rest = ["//wsl.localhost/", "//wsl$/"]
        .iter()
        .find_map(|prefix| lower.starts_with(prefix).then(|| &unified[prefix.len()..]))?;
    let (distribution, inside) = rest.split_once('/').unwrap_or((rest, ""));
    let inside = inside.trim_matches('/');
    (!distribution.is_empty()).then(|| (percent_decode(distribution), format!("/{inside}")))
}

/// 배포판 선택 인자만 바꾸고 나머지 인자는 그대로 둔다. 같은 선택이 여러 번 있으면 하나만 남는다.
pub fn with_distribution(
    args: &[String],
    distribution: &str,
) -> Vec<String> {
    let mut kept = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "-d" || args[index] == "--distribution" {
            index += 1;
        } else if !args[index].starts_with("--distribution=") {
            kept.push(args[index].clone());
        }
        index += 1;
    }
    let mut result = vec!["--distribution".to_string(), distribution.to_string()];
    result.extend(kept);
    result
}

/// 앞쪽 프로그램과 무관하게 터미널의 실행 환경 이름을 정한다.
///
/// Examples
/// --------
/// | 셸                       | platform  | 결과                    |
/// |--------------------------|-----------|-------------------------|
/// | `wsl.exe -d Debian`      | `Windows` | `WSL · Debian`          |
/// | `powershell.exe`         | `Windows` | `PowerShell · Windows`  |
/// | `…\Git\bin\bash.exe`     | `Windows` | `Git Bash · Windows`    |
/// | `/bin/bash`              | `Linux`   | `bash · Linux`          |
pub fn environment_label(
    options: &TerminalOptions,
    cwd: &str,
    platform: &str,
) -> String {
    if is_wsl_shell(options.shell_path) || options.account.is_some_and(|account| account.runtime == Runtime::Wsl) {
        let distribution = distribution_of(options, cwd);
        if distribution.is_empty() { "WSL".to_string() } else { format!("WSL · {distribution}") }
    } else {
        let file = options.shell_path.rsplit(['/', '\\']).next().unwrap_or("");
        let executable = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
        let shell = match executable.to_lowercase().as_str() {
            "pwsh" | "powershell" => "PowerShell".to_string(),
            "cmd" => "Command Prompt".to_string(),
            "bash" if platform == "Windows" => "Git Bash".to_string(),
            _ => executable.to_string(),
        };
        let label =
            [shell.as_str(), platform].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        if label.is_empty() { "System shell".to_string() } else { label }
    }
}

/// Paddock을 실행한 Claude Code 세션이 자식 프로세스에 남기는 표지. 앱이 물려받으면 모든 터미널로 번진다.
/// 터미널 안에서 새로 띄운 Claude는 이 표지를 보고 자신을 하위 세션으로 여겨 대화 저장을 끈다.
/// `CLAUDE_PID`·`CLAUDE_EFFORT`는 바깥 세션의 프로세스 번호와 추론 수준이다. 터미널에서 새로 띄운 Claude는 자기 값으로 덮어쓰지만,
/// Claude 없이 실행한 스크립트에는 이미 끝났을 수 있는 바깥 세션의 값이 보이므로 함께 지운다.
/// 사용자가 정해 두는 Claude 설정 변수(`CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_USE_BEDROCK` 등)는 목록에 없으므로 남는다.
///
/// 다른 Paddock의 터미널 안에서 이 앱을 실행하면 그 터미널의 표지(`PADDOCK_TERMINAL`)도 물려받는다.
/// 그러면 이 앱의 터미널에서 실행한 프로그램이 바깥 Paddock에는 바깥 터미널의 프로그램으로 보인다.
/// 표지가 필요한 터미널(WSL)에는 데몬이 자기 표지를 따로 넣는다.
const INHERITED_SESSION_NAMES: [&str; 12] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "PADDOCK_TERMINAL",
];

/// 환경에서 앱을 실행한 Claude Code 세션의 표지를 지우고, 지운 이름 목록을 돌려준다(없으면 빈 목록).
///
/// Examples
/// --------
/// | 실행한 곳                  | 지우는 것                                  |
/// |----------------------------|--------------------------------------------|
/// | Claude Code 안의 터미널    | `CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, … |
/// | 시작 메뉴·바탕화면         | 없음                                       |
pub fn clear_inherited_session(env: &mut BTreeMap<String, String>) -> Vec<String> {
    INHERITED_SESSION_NAMES.iter().filter(|name| env.remove(**name).is_some()).map(|name| name.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWKWARD: &str = "/tmp/my dir/\"quoted\" $(touch pwned); `id` 'x'";

    #[test]
    fn test_c_r1_shell_args_not_shell_interpreted() {
        // wsl.exe는 폴더를 --cd 뒤의 인자 하나로 받는다.
        let wsl = shell_spec("wsl", Some(AWKWARD), None);
        assert_eq!(wsl.program, "wsl.exe");
        assert_eq!(wsl.args, vec!["--cd".to_string(), AWKWARD.to_string()]);
        assert_eq!(wsl.working_directory, None);

        // 다른 셸은 인자가 아니라 프로세스 작업 폴더로 시작 폴더를 받는다.
        for name in ["powershell", "cmd", "bash", "gitbash"] {
            let spec = shell_spec(name, Some(AWKWARD), None);
            assert_eq!(spec.working_directory.as_deref(), Some(AWKWARD), "{name}");
            assert!(spec.args.iter().all(|arg| !arg.contains("pwned")), "{name}");
        }
    }

    #[test]
    fn test_wsl_distribution_comes_before_the_directory() {
        let spec = shell_spec("wsl", Some("/home/a"), Some("Debian"));
        assert_eq!(spec.args, vec!["-d", "Debian", "--cd", "/home/a"]);
        assert_eq!(shell_spec("wsl", None, None).args, Vec::<String>::new());
    }

    #[test]
    fn test_wsl_folder_splits_the_share_path_into_distribution_and_linux_path() {
        assert_eq!(
            wsl_folder("\\\\wsl.localhost\\Ubuntu\\home\\me\\app"),
            Some(("Ubuntu".to_string(), "/home/me/app".to_string()))
        );
        assert_eq!(wsl_folder("\\\\wsl$\\Debian"), Some(("Debian".to_string(), "/".to_string())));
        assert_eq!(wsl_folder("//WSL.localhost/Ubuntu%2024/srv"), Some(("Ubuntu 24".to_string(), "/srv".to_string())));
        assert_eq!(wsl_folder("C:/Users/me"), None);
        assert_eq!(wsl_folder("/home/me"), None);
    }

    #[test]
    fn test_known_shells_and_passthrough() {
        assert_eq!(shell_spec("powershell", None, None).program, "powershell.exe");
        assert_eq!(shell_spec("bash", None, None).args, vec!["-i"]);
        let other = shell_spec("fish", None, None);
        assert_eq!((other.program.as_str(), other.args.len()), ("fish", 0));
    }

    #[test]
    fn test_every_shell_gets_terminal_env() {
        let spec = shell_spec("bash", None, None);
        assert!(spec.env.contains(&("TERM".to_string(), "xterm-256color".to_string())));
        assert!(spec.env.contains(&("COLORTERM".to_string(), "truecolor".to_string())));
    }

    #[test]
    fn test_default_shell_name() {
        assert_eq!(default_shell_name(true, true), "wsl");
        assert_eq!(default_shell_name(true, false), "powershell");
        assert_eq!(default_shell_name(false, true), "bash");
        assert_eq!(default_shell_name(false, false), "bash");
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn test_terminal_environment_1_a_wsl_folder_selects_its_own_distribution_instead_of_the_launchers() {
        let args = strings(&["-d", "Ubuntu"]);
        let options = TerminalOptions { shell_path: "wsl.exe", shell_args: &args, account: None };
        assert_eq!(distribution_of(&options, "file://wsl.localhost/Debian/home/me/project"), "Debian");
        assert_eq!(distribution_of(&options, "file://wsl$/Ubuntu%2024/home/me"), "Ubuntu 24");
        assert_eq!(
            with_distribution(&strings(&["--cd", "~", "-d", "Ubuntu", "--exec", "bash"]), "Debian"),
            strings(&["--distribution", "Debian", "--cd", "~", "--exec", "bash"])
        );
    }

    #[test]
    fn test_terminal_environment_2_labels_keep_windows_shells_and_wsl_distributions_distinguishable() {
        let label = |shell_path: &str, args: &[&str], platform: &str| {
            let args = strings(args);
            environment_label(&TerminalOptions { shell_path, shell_args: &args, account: None }, "", platform)
        };
        assert_eq!(label("C:\\Windows\\System32\\wsl.exe", &["-d", "Debian"], "Windows"), "WSL · Debian");
        assert_eq!(label("powershell.exe", &[], "Windows"), "PowerShell · Windows");
        assert_eq!(label("C:\\Program Files\\Git\\bin\\bash.exe", &[], "Windows"), "Git Bash · Windows");
        assert_eq!(label("/bin/bash", &[], "Linux"), "bash · Linux");
        assert_eq!(label("", &[], ""), "System shell");
    }

    #[test]
    fn test_terminal_environment_3_adding_a_distribution_keeps_other_options_and_drops_duplicate_flags() {
        assert_eq!(
            with_distribution(&strings(&["--distribution=Ubuntu", "--cd", "/home/me"]), "Debian"),
            strings(&["--distribution", "Debian", "--cd", "/home/me"])
        );
        let args = strings(&["--distribution", "Debian"]);
        assert_eq!(
            distribution_of(&TerminalOptions { shell_path: "", shell_args: &args, account: None }, ""),
            "Debian"
        );
        let account = AccountProfile {
            id: "a".to_string(),
            provider: "claude".to_string(),
            label: "Main".to_string(),
            runtime: Runtime::Wsl,
            wsl_distribution: Some("Ubuntu".to_string()),
        };
        let options = TerminalOptions { shell_path: "", shell_args: &[], account: Some(&account) };
        assert_eq!(distribution_of(&options, ""), "Ubuntu");
    }

    #[test]
    fn test_terminal_environment_4_an_app_started_from_a_claude_terminal_drops_that_sessions_markers_only() {
        let mut env: BTreeMap<String, String> = [
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_CODE_SESSION_ID", "x"),
            ("CLAUDE_PID", "97804"),
            ("CLAUDE_EFFORT", "medium"),
            ("CLAUDE_CONFIG_DIR", "/home/me/.claude"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("PATH", "/bin"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
        assert_eq!(
            clear_inherited_session(&mut env),
            strings(&[
                "CLAUDECODE",
                "CLAUDE_CODE_CHILD_SESSION",
                "CLAUDE_CODE_SESSION_ID",
                "CLAUDE_PID",
                "CLAUDE_EFFORT"
            ])
        );
        let kept: Vec<&str> = env.keys().map(String::as_str).collect();
        assert_eq!(kept, vec!["CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CONFIG_DIR", "PATH"]);
        let mut outer: BTreeMap<String, String> =
            [("PADDOCK_TERMINAL".to_string(), "terminal-outer".to_string()), ("PATH".to_string(), "/bin".to_string())]
                .into_iter()
                .collect();
        assert_eq!(clear_inherited_session(&mut outer), strings(&["PADDOCK_TERMINAL"]));
        assert!(clear_inherited_session(&mut BTreeMap::new()).is_empty());
    }

    #[test]
    fn test_c_cwd_windows_shells_get_the_cwd_report_and_other_shells_are_untouched() {
        let powershell = shell_spec("powershell", None, None);
        assert_eq!(powershell.args[..3], strings(&["-NoLogo", "-NoExit", "-EncodedCommand"])[..]);
        let gitbash = shell_spec("gitbash", None, None);
        assert_eq!(gitbash.args, strings(&["--login", "-i"]));
        assert!(gitbash.env.iter().any(|(name, value)| name == "PROMPT_COMMAND" && value.contains("]7;")));
        let cmd = shell_spec("cmd", None, None);
        assert!(cmd.env.contains(&("PROMPT".to_string(), "$E]7;$P$E\\$P$G".to_string())));
        assert!(shell_spec("wsl", None, None).env.iter().all(|(name, _)| name != "PROMPT"));
        assert_eq!(shell_spec("bash", None, None).args, strings(&["-i"]));
    }
}
