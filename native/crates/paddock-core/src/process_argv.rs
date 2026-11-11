//! Windows 셸(Git Bash·PowerShell·명령 프롬프트)에서 지금 실행 중인 프로그램을 프로세스 목록으로 찾는 순수 규칙.
//!
//! Windows에는 Linux의 `/proc`처럼 "터미널의 앞쪽 프로세스"가 없어, 셸의 자식 프로세스 중 가장 최근에 시작한 것을 앞쪽 프로그램으로 본다.

/// 콘솔을 붙이려고 Windows가 띄우는 도우미 프로세스. 사용자가 실행한 프로그램이 아니다.
const CONSOLE_HELPERS: [&str; 2] = ["conhost.exe", "openconsole.exe"];
/// 다른 프로그램을 대신 실행하는 중간 프로세스(npm의 .cmd·셸 스크립트 실행기). 자식이 있으면 자식을 본다.
const LAUNCHERS: [&str; 4] = ["cmd.exe", "sh.exe", "bash.exe", "env.exe"];
/// 중간 프로세스를 따라 내려가는 최대 깊이. 에이전트가 띄운 하위 도구(ripgrep 등)까지 내려가지 않게 한다.
const N_MAX_DEPTH: usize = 3;

/// 프로세스 목록의 항목 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessItem {
    pub pid: u32,
    pub parent_pid: u32,
    pub command_line: String,
    /// 시작 시각. 크기만 비교한다.
    pub created_at: u64,
}

/// Windows 명령줄을 인자 배열로 나눈다. 큰따옴표로 묶인 부분은 한 인자이고, 따옴표 자체는 뺀다.
///
/// Examples
/// --------
/// | 명령줄                                               | 결과                                                   |
/// |------------------------------------------------------|--------------------------------------------------------|
/// | `"C:\Program Files\nodejs\node.exe" "C:\a b\cli.js"` | `C:\Program Files\nodejs\node.exe`, `C:\a b\cli.js`    |
/// | `bash.exe --login -i`                                | `bash.exe`, `--login`, `-i`                            |
pub fn split_command_line(command_line: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut is_quoted = false;
    let mut has_part = false;
    for character in command_line.chars() {
        if character == '"' {
            is_quoted = !is_quoted;
            has_part = true;
        } else if character.is_whitespace() && !is_quoted {
            if has_part {
                argv.push(std::mem::take(&mut current));
            }
            current.clear();
            has_part = false;
        } else {
            current.push(character);
            has_part = true;
        }
    }
    if has_part {
        argv.push(current);
    }
    argv
}

/// 명령줄 첫 인자의 파일 이름(소문자).
fn executable_name(item: &ProcessItem) -> String {
    let argv = split_command_line(&item.command_line);
    argv.first().map(|first| first.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase()).unwrap_or_default()
}

/// `parent_pid`의 자식 중 가장 최근에 시작한 것. 콘솔 도우미와 자기 자신은 건너뛴다. 시작 시각이 같으면 목록에서 앞선 것이다.
fn newest_child(
    processes: &[ProcessItem],
    parent_pid: u32,
) -> Option<&ProcessItem> {
    let mut newest: Option<&ProcessItem> = None;
    for item in processes.iter().filter(|item| {
        item.parent_pid == parent_pid
            && item.pid != parent_pid
            && !CONSOLE_HELPERS.contains(&executable_name(item).as_str())
    }) {
        if newest.is_none_or(|current| item.created_at > current.created_at) {
            newest = Some(item);
        }
    }
    newest
}

/// Windows 시각(FILETIME: 1601년부터 센 100ns 단위)을 1970년 기준 ms로 바꾼다. 0이거나 1970년 이전이면 None이다.
///
/// Examples
/// --------
/// | FILETIME             | 결과            |
/// |----------------------|-----------------|
/// | `116_444_736_000_000_000` | `Some(0)`  |
/// | `116_444_736_010_000_000` | `Some(1000)` |
/// | `0`                  | None            |
pub fn filetime_to_unix_ms(filetime: u64) -> Option<u64> {
    const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;
    filetime.checked_sub(UNIX_EPOCH_FILETIME).map(|ticks| ticks / 10_000)
}

/// 셸 프로세스에서 지금 실행 중인 프로그램의 명령줄 인자. 실행 중인 프로그램이 없으면 셸 자신의 인자이고, 셸이 목록에 없으면 None이다.
///
/// 셸의 자식 중 가장 최근에 시작한 프로세스를 고르고, 그것이 중간 실행기(cmd·sh)이고 자식이 있으면 한 단계씩 내려간다.
pub fn foreground_argv(
    processes: &[ProcessItem],
    shell_pid: u32,
) -> Option<Vec<String>> {
    foreground_item(processes, shell_pid).map(|chosen| split_command_line(&chosen.command_line))
}

/// `foreground_argv`가 고른 프로세스 자체. 번호와 시작 시각을 알아야 할 때(이어 열 에이전트 대화 찾기) 쓴다.
pub fn foreground_item(
    processes: &[ProcessItem],
    shell_pid: u32,
) -> Option<&ProcessItem> {
    let shell = processes.iter().find(|item| item.pid == shell_pid);
    let mut current = shell.and_then(|_| newest_child(processes, shell_pid));
    let is_launcher = |item: &ProcessItem| LAUNCHERS.contains(&executable_name(item).as_str());
    let mut child = current.filter(|item| is_launcher(item)).and_then(|item| newest_child(processes, item.pid));
    let mut n_depth = 1;
    while let Some(next) = child.filter(|_| n_depth < N_MAX_DEPTH) {
        current = Some(next);
        child = if is_launcher(next) { newest_child(processes, next.pid) } else { None };
        n_depth += 1;
    }
    current.or(shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        pid: u32,
        parent_pid: u32,
        command_line: &str,
        created_at: u64,
    ) -> ProcessItem {
        ProcessItem { pid, parent_pid, command_line: command_line.to_string(), created_at }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn test_c_resume_filetime_becomes_unix_milliseconds() {
        assert_eq!(filetime_to_unix_ms(116_444_736_000_000_000), Some(0));
        assert_eq!(filetime_to_unix_ms(116_444_736_010_000_000), Some(1000));
        assert_eq!(filetime_to_unix_ms(0), None);
        assert_eq!(filetime_to_unix_ms(1_000), None);
    }

    #[test]
    fn test_c_winproc_1_a_quoted_path_is_one_argument() {
        assert_eq!(
            split_command_line("\"C:\\Program Files\\nodejs\\node.exe\" \"C:\\a b\\cli.js\" --x"),
            strings(&["C:\\Program Files\\nodejs\\node.exe", "C:\\a b\\cli.js", "--x"])
        );
        assert_eq!(split_command_line("bash.exe  --login -i"), strings(&["bash.exe", "--login", "-i"]));
        assert_eq!(split_command_line(""), Vec::<String>::new());
    }

    #[test]
    fn test_c_winproc_2_the_newest_child_of_the_shell_wins_and_console_helpers_are_skipped() {
        let processes = vec![
            item(10, 1, "\"C:\\Program Files\\Git\\bin\\bash.exe\" --login -i", 10),
            item(11, 10, "\\??\\C:\\WINDOWS\\system32\\conhost.exe 0x4", 50),
            item(12, 10, "ls.exe", 12),
            item(
                13,
                10,
                "\"C:\\Program Files\\nodejs\\node.exe\" \"C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js\"",
                13,
            ),
        ];
        assert_eq!(
            foreground_argv(&processes, 10),
            Some(strings(&[
                "C:\\Program Files\\nodejs\\node.exe",
                "C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"
            ]))
        );
    }

    #[test]
    fn test_c_winproc_3_launchers_descend_to_their_child_and_without_children_the_shell_itself_is_chosen() {
        let processes = vec![
            item(20, 1, "powershell.exe -NoLogo", 20),
            item(21, 20, "C:\\WINDOWS\\system32\\cmd.exe /d /s /c \"claude.cmd\"", 21),
            item(22, 21, "\"node.exe\" \"C:\\npm\\codex\\bin\\codex.js\"", 22),
            item(23, 22, "rg.exe --files", 23),
        ];
        assert_eq!(foreground_argv(&processes, 20), Some(strings(&["node.exe", "C:\\npm\\codex\\bin\\codex.js"])));
        assert_eq!(foreground_argv(&[item(30, 1, "cmd.exe", 30)], 30), Some(strings(&["cmd.exe"])));
        assert_eq!(foreground_argv(&[], 40), None);
    }
}
