//! Linux: `/proc`에서 셸의 현재 폴더와 터미널 앞쪽 프로세스의 명령줄·환경을 읽는다.
use std::collections::BTreeMap;
use std::path::PathBuf;

use paddock_core::agent_account::{Runtime, parse_environ};

use super::{ProbeTarget, Reading};

pub fn read(targets: &[ProbeTarget]) -> BTreeMap<u32, Reading> {
    targets.iter().filter_map(|target| read_one(target.shell_pid).map(|reading| (target.id, reading))).collect()
}

/// 터미널의 앞쪽 프로세스 그룹 번호. 그룹이 없으면(0 이하) 셸 자신이다.
///
/// `/proc/<pid>/stat`의 여섯째 칸(`tpgid`)이 터미널의 앞쪽 프로세스 그룹이다. 프로그램 이름에 공백·괄호가 있을 수 있어 마지막 `)` 뒤부터 센다.
fn foreground_pid(shell_pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")).ok()?;
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let group: i64 = fields.get(5)?.parse().ok()?;
    Some(if group > 0 { group as u32 } else { shell_pid })
}

/// 프로세스가 시작된 시각(ms, 1970년 기준). `/proc/<pid>/stat`의 시작 시각(부팅 뒤 clock tick)과 `/proc/stat`의 부팅 시각으로 구한다.
fn started_at_of(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // 22번째 칸(starttime)은 마지막 `)` 뒤에서 20번째다.
    let ticks: u64 = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
    // SAFETY: 인자 하나를 받는 시스템 조회이며 포인터를 쓰지 않는다.
    let ticks_per_second = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok().filter(|hz| *hz > 0)?;
    let boot_seconds: u64 = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))
        .and_then(|value| value.trim().parse().ok())?;
    Some(boot_seconds * 1000 + ticks * 1000 / ticks_per_second)
}

fn read_one(shell_pid: u32) -> Option<Reading> {
    let foreground = foreground_pid(shell_pid)?;
    let cmdline = std::fs::read(format!("/proc/{foreground}/cmdline")).ok()?;
    let argv: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    // 다른 사용자로 실행된 프로그램(sudo 등)은 환경을 읽을 수 없다. 그때 계정은 모름으로 둔다.
    let env = std::fs::read(format!("/proc/{foreground}/environ"))
        .ok()
        .map(|bytes| parse_environ(&String::from_utf8_lossy(&bytes), '\0'));
    let cwd = std::fs::read_link(format!("/proc/{shell_pid}/cwd"))
        .ok()
        .map(|path: PathBuf| path.to_string_lossy().into_owned());
    (!argv.is_empty()).then_some(Reading {
        cwd,
        argv,
        env,
        runtime: Runtime::Native,
        distribution: String::new(),
        pid: Some(foreground),
        started_at: started_at_of(foreground),
        session_record: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d37_linux_probe_reads_this_process_like_a_shell_without_children() {
        // 이 시험 프로세스를 "셸"로 본다. 터미널이 없으면 tpgid가 -1이라 프로세스 자신이 앞쪽이다.
        let target = ProbeTarget { id: 1, shell_pid: std::process::id(), wsl: None };
        let readings = read(&[target]);
        let reading = readings.get(&1).expect("own process is readable");
        assert!(!reading.argv.is_empty());
        assert_eq!(reading.cwd.as_deref(), std::env::current_dir().ok().as_deref().and_then(|dir| dir.to_str()));
        assert!(reading.env.is_some());
    }

    #[test]
    fn test_c_d40_the_start_time_of_a_process_is_read_from_proc() {
        let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut child = std::process::Command::new("sleep").arg("5").spawn().expect("sleep starts");
        let started = started_at_of(child.id()).expect("a start time");
        let _ = child.kill();
        let _ = child.wait();
        // clock tick 단위(10 ms)로만 알 수 있어 시험 시작 직전부터 지금까지의 범위에 든다.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        assert!(started + 1000 >= before && started <= now + 1000, "started {started}, window {before}..{now}");
        let target = ProbeTarget { id: 1, shell_pid: std::process::id(), wsl: None };
        let reading = read(&[target]).remove(&1).expect("own process");
        assert_eq!(reading.pid, Some(std::process::id()));
        assert!(reading.started_at.is_some());
    }

    #[test]
    fn test_a_missing_process_is_left_out() {
        let target = ProbeTarget { id: 7, shell_pid: u32::MAX - 1, wsl: None };
        assert!(read(&[target]).is_empty());
    }
}
