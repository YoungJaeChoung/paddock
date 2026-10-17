//! macOS 셸에서 지금 실행 중인 프로그램(터미널의 앞쪽 프로세스)을 `ps` 출력으로 찾는 순수 규칙.
//!
//! macOS에는 Linux의 `/proc`가 없어 `ps`를 실행해야 한다. 실행 자체는 호출자가 하고(`run`), 이 모듈은 어떤 인자로 몇 번 묻고
//! 출력을 어떻게 읽는지만 정한다. 모든 셸을 한 번에 물어 프로세스 실행 횟수가 셸 수와 무관하게 두 번이다.
use std::collections::BTreeMap;

/// 셸의 앞쪽 프로그램.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacForeground {
    pub pid: u32,
    pub argv: Vec<String>,
}

/// `ps -o pid=,<칸>=` 출력을 pid별 나머지 칸 글자로 나눈다. 앞뒤 공백은 지우고, pid가 숫자가 아닌 줄은 버린다.
///
/// Examples
/// --------
/// | 출력                              | 결과                                      |
/// |-----------------------------------|-------------------------------------------|
/// | `  101   205` + `  102     0`     | `101 → "205"`, `102 → "0"`                |
/// | ` 205 node /usr/bin/claude -c`    | `205 → "node /usr/bin/claude -c"`         |
/// | (빈 출력)                         | (빈 표)                                   |
pub fn parse_ps_columns(stdout: &str) -> BTreeMap<u32, String> {
    let mut columns = BTreeMap::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some((pid, rest)) = trimmed.split_once(char::is_whitespace)
            && let Ok(pid) = pid.parse::<u32>()
        {
            columns.insert(pid, rest.trim().to_string());
        }
    }
    columns
}

/// 셸 pid마다 앞쪽 프로그램. 앞쪽 프로세스 그룹이 없으면 셸 자신이고, 명령줄을 읽지 못한 셸(이미 끝남)은 None이다.
///
/// `ps` 실행은 셸 수와 상관없이 두 번(앞쪽 그룹 → 명령줄)이다. `run`은 `ps` 인자를 받아 출력을 돌려준다.
/// macOS에서는 다른 프로세스의 환경을 읽지 않으므로 계정은 판정하지 않는다.
///
/// Examples
/// --------
/// | 셸 pid | `ps -o pid=,tpgid=` | `ps -o pid=,args=` | 결과                                       |
/// |--------|---------------------|--------------------|--------------------------------------------|
/// | 101    | `101 205`           | `205 claude`       | `Some(MacForeground { 205, ["claude"] })`  |
/// | 102    | `102 0`             | `102 /bin/zsh -il` | `Some(MacForeground { 102, ["/bin/zsh", "-il"] })` |
/// | 103    | (줄 없음)           | (줄 없음)          | `None`                                     |
pub fn read_mac_foregrounds(
    shell_pids: &[u32],
    run: &mut dyn FnMut(&[&str]) -> String,
) -> BTreeMap<u32, Option<MacForeground>> {
    let mut result: BTreeMap<u32, Option<MacForeground>> = shell_pids.iter().map(|pid| (*pid, None)).collect();
    if !shell_pids.is_empty() {
        let join = |pids: &[u32]| pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let groups = parse_ps_columns(&run(&["-o", "pid=,tpgid=", "-p", &join(shell_pids)]));
        // 앞쪽 그룹 번호는 그 그룹을 시작한 프로세스의 pid다. 그룹이 없으면(0·-1) 셸 자신을 본다.
        let foreground_of: Vec<(u32, u32)> = shell_pids
            .iter()
            .map(|pid| {
                let group = groups.get(pid).and_then(|text| text.parse::<i64>().ok()).filter(|group| *group > 0);
                (*pid, group.map_or(*pid, |group| group as u32))
            })
            .collect();
        let mut distinct: Vec<u32> = Vec::new();
        for (_, foreground) in &foreground_of {
            if !distinct.contains(foreground) {
                distinct.push(*foreground);
            }
        }
        let command_lines = parse_ps_columns(&run(&["-o", "pid=,args=", "-p", &join(&distinct)]));
        for (pid, foreground) in foreground_of {
            if let Some(args) = command_lines.get(&foreground).filter(|args| !args.is_empty()) {
                let argv = args.split_whitespace().map(str::to_string).collect();
                result.insert(pid, Some(MacForeground { pid: foreground, argv }));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_c_macproc_1_ps_output_is_split_by_pid_and_non_numeric_lines_are_dropped() {
        let expected: BTreeMap<u32, String> = [(101, "205".to_string()), (102, "0".to_string())].into_iter().collect();
        assert_eq!(parse_ps_columns("  101   205\n  102     0\nPID TPGID\n"), expected);
        let one: BTreeMap<u32, String> = [(205, "node /usr/bin/claude -c".to_string())].into_iter().collect();
        assert_eq!(parse_ps_columns(" 205 node /usr/bin/claude -c \n"), one);
        assert!(parse_ps_columns("").is_empty());
    }

    #[test]
    fn test_c_macproc_2_ps_runs_twice_whatever_the_shell_count_and_without_a_foreground_group_the_shell_is_read() {
        let mut calls: Vec<Vec<String>> = Vec::new();
        let mut run = |args: &[&str]| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect());
            if args[1] == "pid=,tpgid=" {
                "101 205\n102 0\n".to_string()
            } else {
                "205 claude --resume\n102 /bin/zsh -il\n".to_string()
            }
        };
        let result = read_mac_foregrounds(&[101, 102, 103], &mut run);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], vec!["-o", "pid=,tpgid=", "-p", "101,102,103"]);
        assert_eq!(calls[1], vec!["-o", "pid=,args=", "-p", "205,102,103"]);
        assert_eq!(
            result[&101],
            Some(MacForeground { pid: 205, argv: vec!["claude".to_string(), "--resume".to_string()] })
        );
        assert_eq!(
            result[&102],
            Some(MacForeground { pid: 102, argv: vec!["/bin/zsh".to_string(), "-il".to_string()] })
        );
        // 이미 끝난 셸은 명령줄이 없어 None이다.
        assert_eq!(result[&103], None);
    }

    #[test]
    fn test_c_macproc_3_without_shells_ps_is_not_run() {
        let mut n_calls = 0;
        let mut run = |_args: &[&str]| {
            n_calls += 1;
            String::new()
        };
        let result = read_mac_foregrounds(&[], &mut run);
        assert_eq!(n_calls, 0);
        assert!(result.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_c_macproc_4_the_real_ps_reads_this_process() {
        let mut run = |args: &[&str]| {
            let output = std::process::Command::new("ps").args(args).output().expect("run ps");
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        let result = read_mac_foregrounds(&[std::process::id()], &mut run);
        assert!(result[&std::process::id()].as_ref().is_some_and(|foreground| !foreground.argv.is_empty()));
    }
}
