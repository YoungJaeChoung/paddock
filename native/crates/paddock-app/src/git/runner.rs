//! git 프로세스 실행: 작업 스레드에서 인자 배열로 띄워 끝날 때까지(최대 20초) 기다리고 출력을 모은다.
//!
//! 셸을 거치지 않고, 표준 입력은 닫으며(묻기 프롬프트가 멈추지 않게), Windows에서는 콘솔 창을 만들지 않는다.
//! 어느 쪽 git인지는 `git_paths`가 정한다 — WSL 폴더는 `wsl.exe`로 배포판 안 git을 부르고 Windows git으로 돌아가지 않는다.
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use paddock_core::git_paths::{GitTarget, command, environment};

use crate::app_log;

/// 상태 읽기·스테이징·diff 같은 짧은 일이 끝나기를 기다리는 최대 시간. 커밋은 훅·서명 입력을 기다릴 수 있어 이 한도를 쓰지 않는다.
///
/// 한도를 넘기면 프로세스를 끝낸다. WSL이면 끝나는 것은 Windows 쪽 `wsl.exe`이고 배포판 안의 git은 스스로 끝나기를 기다린다(감수).
pub const TIME_LIMIT: Duration = Duration::from_secs(20);

static N_LOCAL_RUNS: AtomicUsize = AtomicUsize::new(0);
static N_WSL_RUNS: AtomicUsize = AtomicUsize::new(0);

/// 지금까지 띄운 git 프로세스 수: (이 컴퓨터의 git, `wsl.exe`). 놀 때 호출이 0인지 점검하는 데 쓴다.
pub fn n_runs() -> (usize, usize) {
    (N_LOCAL_RUNS.load(Ordering::Relaxed), N_WSL_RUNS.load(Ordering::Relaxed))
}

/// 실행 결과.
#[derive(Debug)]
pub struct Output {
    /// 종료 코드. 시간 초과로 끝냈거나 신호로 죽었으면 None이다.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub timed_out: bool,
}

impl Output {
    /// 오류 문구에 쓸 첫 줄: 표준 오류의 첫 비지 않은 줄(`fatal: `·`error: ` 머리는 뗀다).
    pub fn first_error_line(&self) -> String {
        let line = self.stderr.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
        line.trim_start_matches("fatal: ").trim_start_matches("error: ").to_string()
    }
}

/// 어느 하위 명령인지(`status`·`add`…)만 적는다. 경로·메시지는 기록하지 않는다.
fn verb_of(args: &[String]) -> &str {
    args.iter().find(|arg| !arg.starts_with('-') && !arg.contains('=')).map_or("-", String::as_str)
}

/// 표준 출력·오류를 끝까지 읽는 스레드.
fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

/// `wsl.exe` 자신의 문구(배포판 없음 등)는 UTF-16LE로 나온다. 그런 바이트면 글자로 풀고, 아니면 UTF-8로 읽는다.
///
/// Examples
/// --------
/// | 바이트                          | 결과                          |
/// |---------------------------------|-------------------------------|
/// | UTF-16LE `There is no…\r\n`     | `There is no…\r\n`            |
/// | UTF-8 `fatal: x`                | `fatal: x`                    |
pub fn decode_text(bytes: &[u8]) -> String {
    let zeros_at_odd = bytes.iter().skip(1).step_by(2).filter(|byte| **byte == 0).count();
    let looks_utf16 = bytes.len() >= 4 && bytes.len().is_multiple_of(2) && zeros_at_odd * 2 > bytes.len() / 2;
    if looks_utf16 {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// `target`의 git으로 `args`를 실행한다. `cwd`는 이 컴퓨터의 git이 실행될 폴더다(WSL은 `--cd`가 정하므로 쓰지 않는다).
/// `limit`가 None이면 끝날 때까지 기다린다. 프로세스를 띄우지 못하면(git이 없음) 오류를 돌려준다.
///
/// WSL에서 git 자신의 종료 코드(0~255) 밖의 값은 `wsl.exe`가 실패한 것이다. 그때는 `wsl.exe`가 쓴 문구(표준 출력이나 오류에
/// UTF-16LE로 나온다)를 `stderr`로 옮겨 이유를 읽을 수 있게 한다.
pub fn run(
    target: &GitTarget,
    cwd: &str,
    args: &[String],
    limit: Option<Duration>,
) -> std::io::Result<Output> {
    let (program, full_args) = command(target, args);
    let is_wsl = matches!(target, GitTarget::Wsl { .. });
    let counter = if is_wsl { &N_WSL_RUNS } else { &N_LOCAL_RUNS };
    counter.fetch_add(1, Ordering::Relaxed);
    app_log::event(&format!("git: {} {}", if is_wsl { "wsl.exe" } else { "local" }, verb_of(args)));
    let mut process = Command::new(&program);
    process.args(&full_args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if !is_wsl {
        process.current_dir(cwd);
    }
    for (name, value) in environment(target, std::env::var("WSLENV").ok().as_deref()) {
        process.env(name, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let mut child = process.spawn()?;
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = limit.map(|limit| Instant::now() + limit);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if deadline.is_some_and(|deadline| Instant::now() >= deadline) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    // 시간 초과로 끝냈으면 읽기 스레드를 기다리지 않는다: git이 띄운 자식(훅 등)이 파이프를 쥔 채 남아 있으면 끝나지 않는다.
    let (stdout, stderr) = if timed_out {
        (Vec::new(), Vec::new())
    } else {
        (
            stdout.and_then(|handle| handle.join().ok()).unwrap_or_default(),
            stderr.and_then(|handle| handle.join().ok()).unwrap_or_default(),
        )
    };
    let code = status.and_then(|status| status.code()).filter(|_| !timed_out);
    let wsl_failed = is_wsl && code.is_some_and(|code| !(0..=255).contains(&code));
    let (stdout, stderr) = if wsl_failed {
        let text = if stderr.is_empty() { &stdout } else { &stderr };
        (Vec::new(), decode_text(text))
    } else {
        (stdout, decode_text(&stderr))
    };
    Ok(Output { code, stdout, stderr, timed_out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn test_c_review_10_wsl_own_utf16_text_is_decoded_and_utf8_text_is_left_alone() {
        let message = "There is no distribution with the supplied name.\r\n";
        assert_eq!(decode_text(&utf16(message)), message);
        assert_eq!(decode_text(&[&[0xff, 0xfe][..], &utf16("Wsl/Service/E_FAIL")].concat()), "Wsl/Service/E_FAIL");
        assert_eq!(decode_text("fatal: not a git repository\n".as_bytes()), "fatal: not a git repository\n");
        assert_eq!(decode_text("한글 파일.txt".as_bytes()), "한글 파일.txt");
        assert_eq!(decode_text(b""), "");
        assert_eq!(decode_text(b"ab"), "ab", "two bytes are too short to be called UTF-16");
    }

    #[test]
    fn test_c_review_8_the_time_limit_ends_a_slow_run_and_no_limit_waits_for_it() {
        let _home = crate::test_support::TempHome::new();
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().to_string();
        let target = paddock_core::git_paths::target_of(&cwd);
        let init = ["init", "-q"].map(String::from);
        assert_eq!(run(&target, &cwd, &init, Some(TIME_LIMIT)).unwrap().code, Some(0));
        let slow = ["-c", "alias.slow=!sleep 3", "slow"].map(String::from);
        let started = Instant::now();
        let limited = run(&target, &cwd, &slow, Some(Duration::from_millis(300))).unwrap();
        assert!(limited.timed_out && limited.code.is_none());
        assert!(started.elapsed() < Duration::from_secs(3), "it did not wait for the sleep");
        let unlimited = run(&target, &cwd, &slow, None).unwrap();
        assert!(!unlimited.timed_out, "without a limit the run is waited for");
        assert_eq!(unlimited.code, Some(0));
    }
}
