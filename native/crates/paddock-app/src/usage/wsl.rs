//! WSL 배포판 사용자의 홈 폴더 읽기: 사용량 기록이 배포판 안 `~/.paddock`·`~/.codex` 아래에 있어서다.
//!
//! `wsl.exe`를 짧게 한 번 띄워 `$HOME`을 묻고, 성공한 값은 5분 동안 다시 쓴다. Windows가 아니거나 배포판을 못 읽으면 None이다.
use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use paddock_core::account_login::{WslMapping, wsl_file_mapping};

/// 홈 폴더를 묻는 `wsl.exe`가 끝나기를 기다리는 최대 시간(WSL이 꺼져 있으면 켜는 데 몇 초 걸린다).
const LOOKUP_LIMIT: Duration = Duration::from_secs(20);
const CACHE: Duration = Duration::from_secs(300);
/// 고정 셸 스크립트(래퍼 설치·`.bashrc` 줄)가 끝나기를 기다리는 최대 시간.
const SCRIPT_LIMIT: Duration = Duration::from_secs(20);
/// 홈 폴더를 읽지 못한 배포판을 다시 묻기 전에 기다리는 시간(WSL이 꺼져 있을 때 `wsl.exe`를 계속 띄우지 않으려는 것).
const FAILURE_CACHE: Duration = Duration::from_secs(30);

/// 같은 배포판을 동시에 묻는 호출이 기다리는 결과 칸.
type Answer = Arc<(Mutex<Option<Option<String>>>, Condvar)>;

/// 배포판 하나의 홈 폴더 조회 상태.
enum Entry {
    /// 읽었다(Linux 경로).
    Known(Instant, String),
    /// 읽지 못했다.
    Failed(Instant),
    /// 지금 `wsl.exe`가 묻는 중이다.
    Asking(Answer),
}

static HOMES: Mutex<BTreeMap<String, Entry>> = Mutex::new(BTreeMap::new());

/// 배포판 이름(None이면 기본 배포판)과 그 사용자 홈의 경로 규칙.
pub fn mapping_of(distribution: Option<&str>) -> Option<(String, WslMapping)> {
    let name = if cfg!(windows) {
        distribution.map(str::to_string).or_else(|| crate::shell_choice::machine_default_distribution().cloned())
    } else {
        None
    };
    name.and_then(|name| {
        let home = home_of(&name)?;
        wsl_file_mapping(&home, &name).map(|mapping| (name, mapping))
    })
}

/// 배포판의 홈 폴더. 5분 안에 읽은 값, 30초 안에 실패한 기록, 같은 배포판을 묻는 중인 호출의 결과를 먼저 쓰고, 아니면 `wsl.exe`를 한 번 띄운다.
fn home_of(distribution: &str) -> Option<String> {
    ask_once(distribution, ask_home)
}

fn ask_once(
    distribution: &str,
    ask: impl FnOnce(&str) -> Option<String>,
) -> Option<String> {
    enum Plan {
        Answer(Option<String>),
        Wait(Answer),
        Ask(Answer),
    }
    let plan = {
        let mut homes = HOMES.lock().ok()?;
        match homes.get(distribution) {
            Some(Entry::Known(at, home)) if at.elapsed() < CACHE => Plan::Answer(Some(home.clone())),
            Some(Entry::Failed(at)) if at.elapsed() < FAILURE_CACHE => Plan::Answer(None),
            Some(Entry::Asking(answer)) => Plan::Wait(answer.clone()),
            _ => {
                let answer: Answer = Arc::new((Mutex::new(None), Condvar::new()));
                homes.insert(distribution.to_string(), Entry::Asking(answer.clone()));
                Plan::Ask(answer)
            }
        }
    };
    match plan {
        Plan::Answer(result) => result,
        Plan::Wait(answer) => {
            let (lock, signal) = &*answer;
            let mut filled = lock.lock().ok()?;
            while filled.is_none() {
                filled = signal.wait(filled).ok()?;
            }
            filled.clone().flatten()
        }
        Plan::Ask(answer) => {
            let found = ask(distribution);
            if let Ok(mut homes) = HOMES.lock() {
                let entry = match &found {
                    Some(home) => Entry::Known(Instant::now(), home.clone()),
                    None => Entry::Failed(Instant::now()),
                };
                homes.insert(distribution.to_string(), entry);
            }
            if let Ok(mut filled) = answer.0.lock() {
                *filled = Some(found.clone());
            }
            answer.1.notify_all();
            found
        }
    }
}

/// 자동 점검이 WSL 홈 대신 쓰는 임시 폴더(Linux 절대 경로)를 정하는 환경 변수. 자동 점검용 빌드만 읽는다.
#[cfg(feature = "automation")]
const TEST_HOME_VARIABLE: &str = "PADDOCK_TEST_WSL_HOME";

/// 자동 점검이 지정한 WSL 임시 홈. 없거나 절대 경로가 아니면 None이다.
#[cfg(feature = "automation")]
fn test_home() -> Option<String> {
    std::env::var(TEST_HOME_VARIABLE).ok().filter(|home| home.starts_with('/') && !home.contains(['\n', '\0']))
}

/// 이 빌드가 WSL 안에 파일을 써도 되는지. 자동 점검용 빌드는 임시 홈(`PADDOCK_TEST_WSL_HOME`)을 지정했을 때만 쓴다.
/// 점검 중에 사용자의 실제 WSL 홈(`~/.paddock`·`~/.bashrc`)이 바뀌지 않게 하려는 것이다. 일반 빌드는 늘 쓸 수 있다.
pub fn writes_allowed() -> bool {
    #[cfg(feature = "automation")]
    let allowed = test_home().is_some();
    #[cfg(not(feature = "automation"))]
    let allowed = true;
    allowed
}

/// `wsl.exe --distribution <배포판> <인자…>`의 출력. 한도 시간 안에 정상으로 끝나지 못하면 None이다.
fn run_wsl(
    distribution: &str,
    args: &[&str],
    limit: Duration,
) -> Option<String> {
    let mut command = Command::new("wsl.exe");
    command
        .args(["--distribution", distribution])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + limit;
    let ended = loop {
        match child.try_wait().ok()? {
            Some(status) => break status.success(),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    if ended { reader.join().ok() } else { None }
}

/// `wsl.exe -d <배포판> --exec /bin/sh -c 'printf %s "$HOME"'`의 출력. 읽지 못하면 None이다.
/// 자동 점검 빌드에서 임시 홈이 지정되어 있으면 그 폴더를 만들고 그 경로를 홈으로 돌려준다.
fn ask_home(distribution: &str) -> Option<String> {
    #[cfg(feature = "automation")]
    let test = test_home();
    #[cfg(not(feature = "automation"))]
    let test: Option<String> = None;
    let text = match test {
        Some(home) => run_wsl(
            distribution,
            &["--exec", "/bin/sh", "-c", "mkdir -p \"$1\" && printf %s \"$1\"", "sh", &home],
            LOOKUP_LIMIT,
        ),
        None => run_wsl(distribution, &["--exec", "/bin/sh", "-c", "printf %s \"$HOME\""], LOOKUP_LIMIT),
    }?;
    let home = text.trim().to_string();
    (home.starts_with('/') && !home.chars().any(|character| character < ' ')).then_some(home)
}

/// WSL 안에서 고정 셸 스크립트 하나를 `HOME=<home>`으로 실행해 출력을 돌려준다.
/// `home`은 `mapping_of`가 돌려준 홈이다. 일반 실행에서는 `$HOME`과 같고, 자동 점검에서는 임시 홈이다.
/// 호출자의 글은 인자로만 넘어가 셸 글이 되지 않는다.
pub fn run_script(
    distribution: &str,
    home: &str,
    script: &str,
    args: &[&str],
) -> Result<String, String> {
    let home_setting = format!("HOME={home}");
    let mut all: Vec<&str> = vec!["--exec", "env", &home_setting, "sh", "-c", script, "paddock-setup"];
    all.extend_from_slice(args);
    run_wsl(distribution, &all, SCRIPT_LIMIT)
        .ok_or_else(|| "The WSL environment could not be prepared. Check that the distribution is available and its home folder is writable.".to_string())
}

/// WSL 안 기본 셸의 `node` 실행 파일 경로. nvm처럼 대화형 셸 설정에서 PATH를 잡는 node도 찾으려고 `bash -ic`로 읽는다.
/// 설정 파일이 찍는 글은 앞쪽 줄이라 마지막 줄만 쓴다. 찾지 못하면 None이다.
pub fn node_of(
    distribution: &str,
    home: &str,
) -> Option<String> {
    let home_setting = format!("HOME={home}");
    let text =
        run_wsl(distribution, &["--exec", "env", &home_setting, "bash", "-ic", "command -v node"], LOOKUP_LIMIT)?;
    let line = text.replace('\r', "").lines().rev().map(str::trim).find(|line| !line.is_empty())?.to_string();
    line.starts_with('/').then_some(line)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn test_c_d74_concurrent_questions_about_one_distribution_start_wsl_once_and_a_failure_is_remembered_briefly() {
        let n_asks = Arc::new(AtomicUsize::new(0));
        let name = "Distro-for-the-concurrency-test";
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let n_asks = n_asks.clone();
                std::thread::spawn(move || {
                    ask_once(name, |_| {
                        n_asks.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(300));
                        None
                    })
                })
            })
            .collect();
        assert!(handles.into_iter().all(|handle| handle.join().unwrap().is_none()));
        assert_eq!(n_asks.load(Ordering::SeqCst), 1, "four callers started wsl.exe once");
        assert_eq!(
            ask_once(name, |_| Some("/home/never-asked".to_string())),
            None,
            "the failure is cached for a short while"
        );
        let ok = "Distro-for-the-success-test";
        assert_eq!(ask_once(ok, |_| Some("/home/me".to_string())).as_deref(), Some("/home/me"));
        assert_eq!(ask_once(ok, |_| None).as_deref(), Some("/home/me"), "a read home is reused");
    }
}
