//! Windows 앱의 WSL 터미널이 WSL 안에서 지금 어느 폴더에 있고 무엇을 실행 중인지 읽는 순수 규칙.
//!
//! Windows에서는 터미널 프로세스(`wsl.exe`)의 현재 폴더·앞쪽 프로그램을 알 수 없어, 터미널마다 붙인 표지(환경 변수)로
//! WSL 안의 셸을 찾고 Linux와 같은 방식(`/proc`)으로 읽는다. 데몬이 `wsl.exe -d <배포판> -e sh -c <script>`로 실행하고,
//! 이 모듈은 그 스크립트 본문과 출력 읽기, 표지를 WSL로 넘기는 환경 변수 값을 정한다.
use std::collections::BTreeMap;

use crate::agent_account::{AccountEnv, parse_environ};

/// WSL 안의 셸에 전달하는 터미널 표지의 환경 변수 이름. 값은 데몬이 정한 터미널 표지 글자다.
pub const MARKER: &str = "PADDOCK_TERMINAL";

/// WSL 터미널에 넘길 `WSLENV` 값. WSL은 Windows 환경 변수 중 `WSLENV`에 이름을 적은 것만 Linux 셸에 넘긴다.
///
/// 사용자가 정해 둔 `WSLENV` 항목을 앞에 그대로 두고, 터미널 표지와 `COLORTERM`을 덧붙인다.
/// `COLORTERM=truecolor`는 터미널이 1600만 색을 그린다는 표시다. 빠지면 WSL 안의 Claude Code·Codex 같은 프로그램이
/// 색을 가까운 기본 16색으로 바꿔 그려, 은은한 짙은 회색 배경이 밝은 회색(#666666) 판으로 보인다.
/// 이미 적힌 이름(`COLORTERM/u`처럼 변환 표시가 붙은 것 포함)은 다시 붙이지 않는다.
///
/// Examples
/// --------
/// | windows_wslenv  | 결과                                        |
/// |-----------------|---------------------------------------------|
/// | (빈 글자)       | `PADDOCK_TERMINAL:COLORTERM`                |
/// | `USERPROFILE/p` | `USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM`  |
/// | `COLORTERM/u`   | `COLORTERM/u:PADDOCK_TERMINAL`              |
pub fn forwarded_wslenv(windows_wslenv: &str) -> String {
    let mut entries: Vec<String> =
        windows_wslenv.split(':').filter(|entry| !entry.is_empty()).map(str::to_string).collect();
    let names: Vec<String> = entries.iter().map(|entry| entry.split('/').next().unwrap_or("").to_string()).collect();
    for name in [MARKER, "COLORTERM"] {
        if !names.iter().any(|existing| existing == name) {
            entries.push(name.to_string());
        }
    }
    entries.join(":")
}

/// 셸 실행 파일이 `wsl.exe`인지. 경로 구분자와 대소문자는 가리지 않는다.
pub fn is_wsl_shell(shell_path: &str) -> bool {
    let name = shell_path.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    name == "wsl" || name == "wsl.exe"
}

/// 표지가 붙은 셸마다 한 줄씩 출력하는 sh 스크립트. 열은 탭으로 나뉜다.
///
/// | 열 | 내용 |
/// |----|------|
/// | 1  | 터미널 표지 |
/// | 2  | Windows 경로로 바꾼 현재 폴더 |
/// | 3  | 앞쪽 프로그램 명령줄(인자 사이 `\x1f`) |
/// | 4  | 앞쪽 프로그램의 계정 환경: `CODEX_HOME`·`CLAUDE_CONFIG_DIR`·`HOME`의 `이름=값`만 `\x1f`로 이은 것(다른 변수는 읽지 않는다) |
/// | 5  | 셸 환경의 `WSL_DISTRO_NAME`. 홈 경로가 같은 두 배포판에서 한쪽에만 등록된 계정을 다른 쪽 터미널에 붙이지 않으려고 읽는다 |
/// | 6  | 앞쪽 프로세스 pid로 된 Claude sessions 기록 본문(없으면 빈 칸). 그 프로세스가 Claude인지는 명령줄로 가린다 |
/// | 7  | 앞쪽 프로세스 pid |
///
/// 셸에서 실행한 프로그램도 표지를 물려받으므로, 부모에게 같은 표지가 없고 터미널이 붙은 프로세스를 셸로 본다.
/// 다른 사용자의 프로세스처럼 읽을 수 없는 항목은 조용히 건너뛴다.
pub fn script() -> String {
    let read = AccountEnv::READ.join("|");
    [
        format!("for f in $(grep -alz '^{MARKER}=' /proc/[0-9]*/environ 2>/dev/null); do"),
        "  p=${f#/proc/}; p=${p%/environ}".to_string(),
        format!("  id=$({{ tr '\\0' '\\n' < \"$f\"; }} 2>/dev/null | sed -n 's/^{MARKER}=//p')"),
        "  s=$(cat /proc/$p/stat 2>/dev/null) || continue".to_string(),
        "  set -- ${s##*) }".to_string(),
        format!("  parent=$({{ tr '\\0' '\\n' < /proc/$2/environ; }} 2>/dev/null | sed -n 's/^{MARKER}=//p')"),
        // 표지를 물려받은 백그라운드 도우미(Codex의 app-server 등)는 터미널이 없어(tty 0) 셸이 아니다.
        "  [ -n \"$id\" ] && [ \"$parent\" != \"$id\" ] && [ \"$5\" != 0 ] || continue".to_string(),
        "  fg=$6; [ \"$fg\" -gt 0 ] 2>/dev/null || fg=$p".to_string(),
        "  cwd=$(wslpath -w \"$(readlink /proc/$p/cwd)\" 2>/dev/null)".to_string(),
        "  argv=$({ tr '\\0' '\\037' < /proc/$fg/cmdline; } 2>/dev/null)".to_string(),
        format!(
            "  env=$({{ tr '\\0' '\\n' < /proc/$fg/environ; }} 2>/dev/null | grep -E '^({read})=' | tr '\\n' '\\037')"
        ),
        "  distro=$({ tr '\\0' '\\n' < \"$f\"; } 2>/dev/null | sed -n 's/^WSL_DISTRO_NAME=//p')".to_string(),
        // Claude가 이 앞쪽 프로세스에 대해 남긴 sessions 기록의 대화 식별자. 계정 폴더는 위에서 읽은 환경(env)에서 얻는다.
        "  cfg=$(printf %s \"$env\" | tr '\\037' '\\n' | sed -n 's/^CLAUDE_CONFIG_DIR=//p')".to_string(),
        "  hm=$(printf %s \"$env\" | tr '\\037' '\\n' | sed -n 's/^HOME=//p')".to_string(),
        // 기록 본문(탭·줄바꿈을 지운 한 줄)을 그대로 내보낸다. 대화형·pid 일치 판정은 읽는 쪽이 Linux 셸과 같은 규칙으로 한다.
        "  sess=$(head -c 4096 \"${cfg:-$hm/.claude}/sessions/$fg.json\" 2>/dev/null | tr -d '\\t\\r\\n')".to_string(),
        "  printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"$id\" \"$cwd\" \"$argv\" \"$env\" \"$distro\" \"$sess\" \"$fg\""
            .to_string(),
        "done".to_string(),
    ]
    .join("\n")
}

/// 스크립트가 읽은 WSL 터미널 하나.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WslTerminal {
    /// Windows 경로(`\\wsl.localhost\…` 또는 `C:\…`). 읽지 못했으면 빈 글자.
    pub cwd: String,
    /// 앞쪽 프로그램 명령줄.
    pub argv: Vec<String>,
    /// 앞쪽 프로그램의 계정 환경(`CODEX_HOME`·`CLAUDE_CONFIG_DIR`·`HOME` 중 읽힌 것).
    pub env: AccountEnv,
    /// 셸의 WSL 배포판 이름.
    pub distribution: String,
    /// 앞쪽 프로세스(pid)의 Claude sessions 기록 본문.
    pub session_record: String,
    /// 앞쪽 프로세스 pid(글자).
    pub pid: String,
}

/// 스크립트 출력을 터미널 표지별 `WslTerminal`로 바꾼다.
/// 넷째~일곱째 칸이 없는 줄도 받고(없는 칸은 빈 값), 형식이 맞지 않는 줄은 버린다.
///
/// Examples
/// --------
/// | 출력 줄                                         | 결과                                                    |
/// |-------------------------------------------------|---------------------------------------------------------|
/// | `t1` `\\wsl.localhost\Ubuntu\home\me\app` `claude` | `t1 → { cwd: …\app, argv: ["claude"] }`              |
/// | `t2` `` `-bash`                                 | `t2 → { cwd: "", argv: ["-bash"] }`                     |
/// | `broken line`                                   | 버려진다                                                |
pub fn parse(output: &str) -> BTreeMap<String, WslTerminal> {
    let mut terminals = BTreeMap::new();
    for line in output.lines() {
        let columns: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        let column = |index: usize| columns.get(index).copied().unwrap_or("");
        if columns.len() >= 3 && !columns[0].is_empty() && !columns[0].starts_with('#') {
            terminals.insert(
                columns[0].to_string(),
                WslTerminal {
                    cwd: column(1).to_string(),
                    argv: column(2).split('\x1f').filter(|arg| !arg.is_empty()).map(str::to_string).collect(),
                    env: parse_environ(column(3), '\x1f'),
                    distribution: column(4).to_string(),
                    session_record: column(5).to_string(),
                    pid: column(6).to_string(),
                },
            );
        }
    }
    terminals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_account::AccountEnv;

    fn env(pairs: &[(&str, &str)]) -> AccountEnv {
        let mut env = AccountEnv::default();
        for (name, value) in pairs {
            match *name {
                "HOME" => env.home = Some(value.to_string()),
                "CODEX_HOME" => env.codex_home = Some(value.to_string()),
                _ => env.claude_config_dir = Some(value.to_string()),
            }
        }
        env
    }

    fn found(
        cwd: &str,
        argv: &[&str],
        env: AccountEnv,
    ) -> WslTerminal {
        WslTerminal {
            cwd: cwd.to_string(),
            argv: argv.iter().map(|arg| arg.to_string()).collect(),
            env,
            distribution: String::new(),
            session_record: String::new(),
            pid: String::new(),
        }
    }

    #[test]
    fn test_c_wsl_1_only_wsl_exe_shells_are_wsl_terminals() {
        assert!(is_wsl_shell("C:\\Windows\\System32\\wsl.exe"));
        assert!(is_wsl_shell("wsl"));
        assert!(!is_wsl_shell("C:\\Program Files\\Git\\bin\\bash.exe"));
        assert!(!is_wsl_shell(""));
    }

    #[test]
    fn test_c_wsl_2_cwd_and_command_line_are_read_per_terminal_and_broken_lines_dropped() {
        let output = "t1\t\\\\wsl.localhost\\Ubuntu\\home\\me\\app\tnode\x1f/usr/bin/claude\x1f\r\nt2\t\t-bash\x1f\nbroken line\n";
        let parsed = parse(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed["t1"],
            found("\\\\wsl.localhost\\Ubuntu\\home\\me\\app", &["node", "/usr/bin/claude"], AccountEnv::default())
        );
        assert_eq!(parsed["t2"], found("", &["-bash"], AccountEnv::default()));
        assert!(parse("").is_empty());
    }

    #[test]
    fn test_c_wsl_3_the_fourth_column_keeps_only_codex_home_claude_config_dir_and_home() {
        let output = "t1\t\tcodex\x1f\tHOME=/home/me\x1fCODEX_HOME=/home/me/.paddock/agent-profiles/a/codex\x1fOPENAI_API_KEY=secret\x1f\r\n";
        assert_eq!(
            parse(output)["t1"],
            found(
                "",
                &["codex"],
                env(&[("HOME", "/home/me"), ("CODEX_HOME", "/home/me/.paddock/agent-profiles/a/codex")])
            )
        );
        // 스크립트는 앞쪽 프로그램의 환경에서 세 이름만 골라 출력한다.
        assert!(script().contains("grep -E '^(CLAUDE_CONFIG_DIR|CODEX_HOME|HOME)='"));
        assert!(script().contains("/proc/$fg/environ"));
    }

    #[test]
    fn test_c_wsl_4_the_fifth_column_is_the_distribution_used_to_compare_accounts() {
        let output = "t1\t\tcodex\x1f\tHOME=/home/me\x1f\tUbuntu-22.04\r\n";
        let mut expected = found("", &["codex"], env(&[("HOME", "/home/me")]));
        expected.distribution = "Ubuntu-22.04".to_string();
        assert_eq!(parse(output)["t1"], expected);
        assert!(script().contains("WSL_DISTRO_NAME="));
    }

    #[test]
    fn test_c_wsl_forward_user_wslenv_is_kept_and_the_marker_and_colorterm_are_added() {
        assert_eq!(forwarded_wslenv(""), "PADDOCK_TERMINAL:COLORTERM");
        assert_eq!(forwarded_wslenv("USERPROFILE/p"), "USERPROFILE/p:PADDOCK_TERMINAL:COLORTERM");
        assert_eq!(forwarded_wslenv("COLORTERM/u:PADDOCK_TERMINAL"), "COLORTERM/u:PADDOCK_TERMINAL");
    }

    #[test]
    fn test_c_wsl_5_the_sixth_and_seventh_columns_are_the_sessions_record_and_pid() {
        let record = r#"{"pid":55,"sessionId":"0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10","kind":"interactive"}"#;
        let line = format!("t1\t\tclaude\x1f\tHOME=/home/me\x1f\tUbuntu\t{record}\t55\r\n");
        let parsed = parse(&line);
        assert_eq!(parsed["t1"].session_record, record);
        assert_eq!(parsed["t1"].pid, "55");
        #[cfg(unix)]
        read_sessions_record_with_a_real_shell(record);
    }

    /// 스크립트에서 기록을 읽는 줄들만 떼어 실제 sh로 돌린다: 계정 폴더가 있으면 그 폴더, 없으면 HOME/.claude의 기록을 읽는다.
    #[cfg(unix)]
    fn read_sessions_record_with_a_real_shell(record: &str) {
        let script = script();
        let lines: Vec<&str> = script
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                line.starts_with(' ')
                    && (trimmed.starts_with("cfg=") || trimmed.starts_with("hm=") || trimmed.starts_with("sess="))
            })
            .collect();
        assert_eq!(lines.len(), 3);
        let home = std::env::temp_dir().join(format!("paddock-wsl-sess-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude").join("sessions")).unwrap();
        std::fs::create_dir_all(home.join("account").join("sessions")).unwrap();
        std::fs::write(home.join(".claude").join("sessions").join("55.json"), record).unwrap();
        std::fs::write(
            home.join("account").join("sessions").join("55.json"),
            "{\"pid\":55,\n\"sessionId\":\"22222222-2222-4222-8222-222222222222\",\"kind\":\"sdk\"}",
        )
        .unwrap();
        let read = |environment: &str| {
            let program = format!("fg=55; env='{environment}'; {}\nprintf %s \"$sess\"", lines.join("\n"));
            let output = std::process::Command::new("sh").args(["-c", &program]).output().expect("run sh");
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        let home_text = home.display();
        assert_eq!(read(&format!("HOME={home_text}\x1f")), record);
        // 비대화형(sdk) 기록도 본문은 그대로 오고, 거르는 일은 읽는 쪽 판정이 한다. 줄바꿈은 지워진다.
        assert_eq!(
            read(&format!("CLAUDE_CONFIG_DIR={home_text}/account\x1fHOME={home_text}\x1f")),
            "{\"pid\":55,\"sessionId\":\"22222222-2222-4222-8222-222222222222\",\"kind\":\"sdk\"}"
        );
        assert_eq!(read(&format!("HOME={home_text}/nowhere\x1f")), "");
        let _ = std::fs::remove_dir_all(&home);
    }
}
