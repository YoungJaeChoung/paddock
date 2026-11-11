//! Finder·Dock·`open`으로 연 macOS 앱의 PATH를 사용자의 로그인 셸 PATH로 채운다.
//!
//! 이렇게 연 앱은 터미널의 환경이 아니라 launchd의 기본 환경(`PATH=/usr/bin:/bin:/usr/sbin:/sbin`)을 받아,
//! Homebrew나 `~/.local/bin`(claude 등)에 있는 프로그램을 앱이 직접 띄우는 명령(`bash -ic` 사용량 조회, git)과
//! 앱이 데몬에 실어 보내는 새 터미널 환경이 찾지 못한다. 앱이 시작할 때 사용자의 로그인 셸을 한 번 띄워
//! 그 셸이 만든 PATH를 읽어 앱 자신의 PATH로 쓴다. 터미널에서 띄운 앱은 이미 그 PATH를 물려받으므로 건드리지 않는다.
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// 로그인 셸이 PATH를 알려 줄 때까지 기다리는 최대 시간. 넘으면 PATH를 바꾸지 않는다.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// 셸 출력에서 PATH 값을 감싸는 표지. 셸 초기화 파일이 내는 다른 글과 섞이지 않게 한다.
mod marker {
    pub const BEGIN: &str = "__PADDOCK_PATH_BEGIN__";
    pub const END: &str = "__PADDOCK_PATH_END__";
}

/// launchd가 직접 연 앱이면 로그인 셸의 PATH를 이 프로세스의 PATH로 삼는다. 읽지 못하면 PATH를 그대로 둔다.
///
/// 환경 변수를 바꾸므로 다른 스레드가 환경을 읽기 전, 프로세스 맨 처음에 한 번만 부른다.
///
/// Examples
/// --------
/// | 앱을 연 방법              | 부모 프로세스 | 결과                                |
/// |---------------------------|---------------|-------------------------------------|
/// | Finder·Dock·`open`        | launchd(1)    | PATH = 로그인 셸 PATH + 기존 항목   |
/// | 터미널에서 실행 파일 실행 | 셸            | 그대로                              |
pub fn adopt_login_shell_path() {
    // SAFETY: `getppid`는 인자도 메모리 접근도 없는 호출이다.
    let started_by_launchd = unsafe { libc::getppid() } == 1;
    if started_by_launchd {
        let shell = paddock_core::shell_catalog::default_shell_name(false, false);
        if let Some(login_path) = read_login_shell_path(shell) {
            let current = std::env::var("PATH").ok();
            // SAFETY: 호출자가 다른 스레드가 생기기 전에 부르므로 환경을 동시에 읽는 쪽이 없다.
            unsafe { std::env::set_var("PATH", merge_path(&login_path, current.as_deref())) };
        }
    }
}

/// 로그인 셸(`-l`)을 대화형(`-i`)으로 띄워 그 셸의 PATH를 읽는다. `.zprofile`뿐 아니라 `.zshrc`에서 PATH를 잡는 사용자도 있어서 둘 다 읽게 한다.
/// 셸이 실패하거나 `SHELL_TIMEOUT` 안에 답하지 않으면 None이다.
fn read_login_shell_path(shell: &str) -> Option<String> {
    let script = format!("echo {}; printenv PATH; echo {}", marker::BEGIN, marker::END);
    run_for_marked_path(shell, &["-l", "-i", "-c", &script])
}

/// `program`을 자기 프로세스 그룹으로 띄워 출력의 표지 사이 값을 읽고, 끝나면 그 그룹 전체를 끝낸다.
///
/// 닫힘(EOF)이 아니라 끝 표지를 읽는 즉시 답한다: 셸 초기화 파일이 `(cmd &)`처럼 출력 통로를 쥔 백그라운드 프로세스를
/// 남기면 EOF가 그 프로세스가 끝날 때까지 오지 않아 매번 시간 한도를 다 기다리게 된다.
/// 그룹 전체를 끝내므로 그런 백그라운드 프로세스도 남지 않는다. 표지가 오기 전에 한도가 지나면 None이다.
fn run_for_marked_path(
    program: &str,
    args: &[&str],
) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            output.push_str(&line);
            output.push('\n');
            if line.contains(marker::END) {
                break;
            }
        }
        let _ = sender.send(marked_path(&output));
    });
    let path = receiver.recv_timeout(SHELL_TIMEOUT).ok().flatten();
    // 답을 받았든 시간이 지났든 셸과 그 자손을 끝내고 셸을 거둔다. 그룹 번호는 셸의 프로세스 번호와 같다.
    // SAFETY: 프로세스 그룹 번호와 신호만 넘기며 메모리를 건드리지 않는다.
    unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
    let _ = child.wait();
    path
}

/// 셸 출력에서 표지 사이의 PATH 값을 꺼낸다. 표지가 없거나 값이 비면 None이다.
///
/// Examples
/// --------
/// | 출력                                                           | 결과          |
/// |----------------------------------------------------------------|---------------|
/// | `welcome\n__PADDOCK_PATH_BEGIN__\n/a:/b\n__PADDOCK_PATH_END__\n` | `Some("/a:/b")` |
/// | `__PADDOCK_PATH_BEGIN__\n\n__PADDOCK_PATH_END__\n`             | None          |
/// | `welcome\n`                                                    | None          |
fn marked_path(output: &str) -> Option<String> {
    let after_begin = output.split_once(marker::BEGIN)?.1;
    let between = after_begin.split_once(marker::END)?.0.trim();
    (!between.is_empty()).then(|| between.to_string())
}

/// 로그인 셸의 PATH 항목을 앞에 두고, 현재 PATH에만 있는 항목을 뒤에 붙인다. 같은 항목은 한 번만 둔다.
///
/// Examples
/// --------
/// | 로그인 셸 PATH      | 현재 PATH           | 결과                       |
/// |---------------------|---------------------|----------------------------|
/// | `/opt/bin:/usr/bin` | `/usr/bin:/bin`     | `/opt/bin:/usr/bin:/bin`   |
/// | `/opt/bin`          | (없음)              | `/opt/bin`                 |
fn merge_path(
    login_path: &str,
    current_path: Option<&str>,
) -> String {
    let mut entries: Vec<&str> = Vec::new();
    for entry in login_path.split(':').chain(current_path.unwrap_or("").split(':')) {
        if !entry.is_empty() && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marked_path_skips_the_noise_printed_by_shell_startup_files() {
        let output = "welcome\n__PADDOCK_PATH_BEGIN__\n/a:/b\n__PADDOCK_PATH_END__\ngoodbye\n";
        assert_eq!(marked_path(output).as_deref(), Some("/a:/b"));
    }

    #[test]
    fn test_marked_path_is_none_without_markers_or_with_an_empty_value() {
        assert_eq!(marked_path("welcome\n"), None);
        assert_eq!(marked_path("__PADDOCK_PATH_BEGIN__\n\n__PADDOCK_PATH_END__\n"), None);
        assert_eq!(marked_path("__PADDOCK_PATH_BEGIN__\n/a\n"), None);
    }

    #[test]
    fn test_merge_path_puts_login_entries_first_and_keeps_current_only_entries() {
        assert_eq!(merge_path("/opt/bin:/usr/bin", Some("/usr/bin:/bin")), "/opt/bin:/usr/bin:/bin");
        assert_eq!(merge_path("/opt/bin", None), "/opt/bin");
        assert_eq!(merge_path("/a::/b", Some("/b:")), "/a:/b");
    }

    #[test]
    fn test_a_background_process_holding_the_output_neither_delays_the_answer_nor_outlives_it() {
        let marker_file = std::env::temp_dir().join(format!("paddock-login-env-{}", std::process::id()));
        let script = format!(
            "(sleep 1; touch '{}') & echo {}; echo /x:/y; echo {}",
            marker_file.display(),
            marker::BEGIN,
            marker::END
        );
        let started = std::time::Instant::now();
        let path = run_for_marked_path("/bin/sh", &["-c", &script]);
        assert_eq!(path.as_deref(), Some("/x:/y"));
        assert!(started.elapsed() < Duration::from_millis(900), "{:?}", started.elapsed());
        // 백그라운드 프로세스가 살아 있었다면 1초 뒤 표지 파일을 만든다.
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker_file.exists(), "the background process was not ended");
    }

    #[test]
    fn test_a_shell_that_never_prints_the_markers_gives_none_when_it_exits() {
        assert_eq!(run_for_marked_path("/bin/sh", &["-c", "echo hello"]), None);
    }

    #[test]
    fn test_the_login_shell_reports_a_path_with_the_system_directories() {
        let shell = paddock_core::shell_catalog::default_shell_name(false, false);
        let path = read_login_shell_path(shell).expect("the login shell prints its PATH");
        assert!(path.split(':').any(|entry| entry == "/usr/bin"), "{path}");
    }
}
