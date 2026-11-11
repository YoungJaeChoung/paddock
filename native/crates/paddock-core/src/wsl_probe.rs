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
/// | 8  | 앞쪽 프로세스의 시작 tick(`/proc/<pid>/stat` 22번째 칸, 부팅 뒤 clock tick). 같은 pid의 다른 프로세스와 구분하는 표지로만 쓴다 |
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
        // 이 줄 뒤로는 위치 인자($2·$5·$6)를 쓰지 않으므로 앞쪽 프로세스의 stat으로 덮어써도 된다.
        "  st=$(cat /proc/$fg/stat 2>/dev/null); set -- ${st##*) }; ticks=${20:-}".to_string(),
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
        "  printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"$id\" \"$cwd\" \"$argv\" \"$env\" \"$distro\" \"$sess\" \"$fg\" \"$ticks\""
            .to_string(),
        "done".to_string(),
    ]
    .join("\n")
}

/// WSL 안에서 에이전트 CLI에 SIGTERM을 보내는 고정 스크립트. 인자는 프로세스 번호, 터미널 표지, 시작 tick(없으면 빈 글자)이다.
/// 그 프로세스의 환경에 이 터미널의 표지가 있고, 명령줄이 claude이고, 시작 tick이 요청과 같을 때만 신호를 보낸다(다르면 종료 코드 3).
/// 프로세스 번호가 재사용되어 같은 번호의 다른 프로세스가 된 경우를 시작 tick이 걸러낸다.
pub const STOP_SCRIPT: &str = r#"set -eu
pid=$1
marker=$2
expected=${3:-}
env_text=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null) || exit 0
printf '%s\n' "$env_text" | grep -qxF "PADDOCK_TERMINAL=$marker" || exit 3
tr '\0' '\n' < "/proc/$pid/cmdline" 2>/dev/null | grep -qE '(^|/)claude$' || exit 3
stat=$(cat "/proc/$pid/stat" 2>/dev/null) || exit 0
set -- ${stat##*) }
shift 19
[ -z "$expected" ] || [ "$1" = "$expected" ] || exit 3
kill -TERM "$pid""#;

/// WSL 가상 머신의 메모리와 터미널별 메모리를 읽는 sh 스크립트. 열은 탭으로 나뉜다.
///
/// | 줄 | 내용 |
/// |----|------|
/// | `#memory` | `MemTotal`·`MemAvailable`·`SwapTotal`·`SwapFree`(kB) 한 줄(맨 앞) |
/// | `#rss` | 터미널 표지, 고유 상주 페이지 수(상주 − 공유). 표지가 붙은 프로세스마다 한 줄(셸이 아닌 자식 포함) |
/// | `#shell` | 터미널 표지. 셸 자신(부모에게 같은 표지가 없고 터미널이 붙은 프로세스)이 있는 터미널마다 한 줄 |
///
/// 내장 명령 위주로 읽는다. 어느 터미널이 큰지 비교하는 용도라 파일 캐시·공유 라이브러리를 나눠 쓰는 몫은 뺀다.
pub fn memory_script() -> String {
    [
        "{ while read -r key value _; do case $key in MemTotal:) t=$value;; MemAvailable:) a=$value;; SwapTotal:) w=$value;; SwapFree:) f=$value;; esac; done < /proc/meminfo; printf '#memory\\t%s\\t%s\\t%s\\t%s\\n' \"$t\" \"$a\" \"$w\" \"$f\"; } 2>/dev/null".to_string(),
        format!("for f in $(grep -alz '^{MARKER}=' /proc/[0-9]*/environ 2>/dev/null); do"),
        "  p=${f#/proc/}; p=${p%/environ}".to_string(),
        format!("  id=$({{ tr '\\0' '\\n' < \"$f\"; }} 2>/dev/null | sed -n 's/^{MARKER}=//p')"),
        "  { read -r _ rss shared _ < /proc/$p/statm; } 2>/dev/null && [ -n \"$id\" ] && printf '#rss\\t%s\\t%s\\n' \"$id\" \"$((rss - shared))\"".to_string(),
        "  s=$(cat /proc/$p/stat 2>/dev/null) || continue".to_string(),
        "  set -- ${s##*) }".to_string(),
        format!("  parent=$({{ tr '\\0' '\\n' < /proc/$2/environ; }} 2>/dev/null | sed -n 's/^{MARKER}=//p')"),
        // 표지를 물려받은 백그라운드 도우미는 터미널이 없어(tty 0) 셸이 아니다.
        "  [ -n \"$id\" ] && [ \"$parent\" != \"$id\" ] && [ \"$5\" != 0 ] && printf '#shell\\t%s\\n' \"$id\"".to_string(),
        "done".to_string(),
    ]
    .join("\n")
}

/// 메모리 스크립트 출력에서 읽은 값.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedMemory {
    /// 바이트.
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// 터미널 표지별 고유 상주 메모리(바이트).
    pub terminals: BTreeMap<String, u64>,
    /// 셸이 살아 있는 터미널 표지.
    pub shells: Vec<String>,
}

/// WSL 상주 페이지 크기. WSL이 도는 x86_64·arm64 리눅스는 모두 4 KiB다.
const PAGE_BYTES: u64 = 4096;

/// 메모리 스크립트 출력을 읽는다. `#memory` 줄이 없거나 값이 숫자가 아니거나 전체가 0이면 None이다(WSL을 읽지 못한 경우).
///
/// Examples
/// --------
/// | 출력                                                          | 결과                                                                        |
/// |---------------------------------------------------------------|-----------------------------------------------------------------------------|
/// | `#memory 100 40 10 4` · `#rss t1 3` · `#rss t1 2` · `#shell t1` | total 102400·available 40960·swap 10240·사용 6144, `t1` 20480, 셸 `t1`      |
/// | `t1 … bash`(메모리 줄 없음)                                   | None                                                                        |
pub fn parse_memory(output: &str) -> Option<ParsedMemory> {
    let mut memory: Option<(u64, u64, u64, u64)> = None;
    let mut pages: BTreeMap<String, u64> = BTreeMap::new();
    let mut shells = Vec::new();
    for line in output.lines() {
        let columns: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        let number = |index: usize| columns.get(index).and_then(|text| text.trim().parse::<u64>().ok());
        match columns.first().copied() {
            Some("#memory") => {
                if let (Some(total), Some(available), Some(swap), Some(swap_free)) =
                    (number(1), number(2), number(3), number(4))
                    && total > 0
                {
                    memory = Some((total, available, swap, swap_free));
                }
            }
            Some("#rss") => {
                if let (Some(id), Some(count)) = (columns.get(1).filter(|id| !id.is_empty()), number(2)) {
                    *pages.entry((*id).to_string()).or_insert(0) += count;
                }
            }
            Some("#shell") => {
                if let Some(id) = columns.get(1).filter(|id| !id.is_empty()) {
                    shells.push((*id).to_string());
                }
            }
            _ => {}
        }
    }
    memory.map(|(total, available, swap, swap_free)| ParsedMemory {
        total: total * 1024,
        available: available * 1024,
        swap_total: swap * 1024,
        swap_used: swap.saturating_sub(swap_free) * 1024,
        terminals: pages.into_iter().map(|(id, count)| (id, count * PAGE_BYTES)).collect(),
        shells,
    })
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
    /// 앞쪽 프로세스의 시작 tick(글자). 읽지 못했으면 빈 글자.
    pub start_ticks: String,
}

/// 스크립트 출력을 터미널 표지별 `WslTerminal`로 바꾼다.
/// 넷째~여덟째 칸이 없는 줄도 받고(없는 칸은 빈 값), 형식이 맞지 않는 줄은 버린다.
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
                    start_ticks: column(7).to_string(),
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
            start_ticks: String::new(),
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

    /// 종료 스크립트를 실제 sh로 돌린다: 표지·명령줄·시작 tick이 모두 맞을 때만 신호를 보낸다.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_c_d77_the_stop_script_signals_only_a_claude_with_the_marker_and_the_start_tick() {
        let dir = std::env::temp_dir().join(format!("paddock-stop-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("claude");
        std::fs::copy("/bin/sleep", &fake).unwrap();
        let mut child = std::process::Command::new(&fake).arg("30").env(MARKER, "mk1").spawn().unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        let tick = stat.rsplit_once(')').unwrap().1.split_whitespace().nth(19).unwrap().to_string();
        let pid = child.id().to_string();
        let run = |marker: &str, expected: &str| {
            std::process::Command::new("sh")
                .args(["-c", STOP_SCRIPT, "paddock-stop", &pid, marker, expected])
                .status()
                .unwrap()
                .code()
        };
        // 다른 표지·다른 시작 tick은 신호 없이 종료 코드 3이고, 프로세스는 그대로다.
        assert_eq!(run("other", &tick), Some(3));
        assert_eq!(run("mk1", "1"), Some(3));
        assert!(child.try_wait().unwrap().is_none());
        // 모두 맞으면 신호를 보내 끝난다.
        assert_eq!(run("mk1", &tick), Some(0));
        let status = child.wait().unwrap();
        assert!(!status.success());
        // 이름이 claude가 아닌 프로세스는 표지가 같아도 건드리지 않는다.
        let mut other = std::process::Command::new("sleep").arg("30").env(MARKER, "mk1").spawn().unwrap();
        assert_eq!(
            std::process::Command::new("sh")
                .args(["-c", STOP_SCRIPT, "paddock-stop", &other.id().to_string(), "mk1", ""])
                .status()
                .unwrap()
                .code(),
            Some(3)
        );
        assert!(other.try_wait().unwrap().is_none());
        let _ = other.kill();
        let _ = other.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_c_wsl_6_the_eighth_column_is_the_start_tick_of_the_front_process() {
        let line = "t1\t\tclaude\x1f\tHOME=/home/me\x1f\tUbuntu\t\t55\t123456\r\n";
        assert_eq!(parse(line)["t1"].start_ticks, "123456");
        // 여덟째 칸이 없는 옛 출력도 받는다.
        assert_eq!(parse("t1\t\tclaude\x1f\n")["t1"].start_ticks, "");
        #[cfg(target_os = "linux")]
        read_start_tick_with_a_real_shell();
    }

    /// 스크립트에서 시작 tick을 읽는 줄만 떼어 실제 sh로 돌린다: `/proc/<pid>/stat` 22번째 칸과 같아야 한다(프로그램 이름에 공백·괄호가 있어도).
    #[cfg(target_os = "linux")]
    fn read_start_tick_with_a_real_shell() {
        let script = script();
        let line = script.lines().find(|line| line.trim_start().starts_with("st=")).expect("the tick line");
        let mut child = std::process::Command::new("sleep").arg("20").spawn().unwrap();
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
        let expected = stat.rsplit_once(')').unwrap().1.split_whitespace().nth(19).unwrap().to_string();
        let program = format!("fg={}; {line}\nprintf %s \"$ticks\"", child.id());
        let output = std::process::Command::new("sh").args(["-c", &program]).output().expect("run sh");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        // 읽을 수 없는 프로세스는 빈 칸이다.
        let program = format!("fg=4194999; {line}\nprintf %s \"$ticks\"");
        let output = std::process::Command::new("sh").args(["-c", &program]).output().expect("run sh");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
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
    #[test]
    fn test_c_d76_parse_memory_reads_the_vm_totals_and_sums_the_resident_pages_per_terminal() {
        let output =
            "#memory\t100\t40\t10\t4\n#rss\tt1\t3\n#rss\tt1\t2\n#rss\tt2\t1\n#shell\tt1\nt9\t\\\\wsl\tclaude\n";
        let parsed = parse_memory(output).unwrap();
        assert_eq!(
            (parsed.total, parsed.available, parsed.swap_total, parsed.swap_used),
            (102_400, 40_960, 10_240, 6_144)
        );
        assert_eq!(parsed.terminals.get("t1"), Some(&20_480));
        assert_eq!(parsed.terminals.get("t2"), Some(&4_096));
        assert_eq!(parsed.shells, vec!["t1".to_string()]);
    }

    #[test]
    fn test_c_d76_parse_memory_is_none_without_a_usable_memory_line() {
        assert_eq!(parse_memory("t1\t\tbash\n"), None);
        assert_eq!(parse_memory("#memory\tx\t1\t1\t1\n"), None);
        assert_eq!(parse_memory("#memory\t0\t0\t0\t0\n"), None);
        assert_eq!(parse_memory(""), None);
    }

    #[test]
    fn test_c_d76_the_memory_script_reads_only_proc_and_prints_the_three_line_kinds() {
        let script = memory_script();
        for needle in ["/proc/meminfo", "#memory", "#rss", "#shell", "PADDOCK_TERMINAL"] {
            assert!(script.contains(needle), "{needle}");
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn test_c_d76_the_memory_script_runs_under_sh_and_reads_this_machines_meminfo_and_a_marked_process() {
        let mut child = std::process::Command::new("sleep").arg("20").env(MARKER, "test-1").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let output = std::process::Command::new("sh").args(["-c", &memory_script()]).output().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        let parsed = parse_memory(&String::from_utf8_lossy(&output.stdout)).expect("meminfo is readable");
        assert!(parsed.total > 0 && parsed.available <= parsed.total);
        assert!(parsed.terminals.get("test-1").is_some_and(|bytes| *bytes > 0), "{:?}", parsed.terminals);
        assert!(!parsed.shells.contains(&"test-1".to_string()), "a process without a terminal is not a shell");
    }
}
