//! Codex 공식 CLI에게 계정 한도를 묻는 일: `codex app-server`를 자식 프로세스로 띄워 두 메시지만 주고받는다.
//!
//! 조회 하나는 작업 스레드에서 도는 짧은 수명(최대 15초)의 자식 프로세스다. 출력은 1 MB까지만 받고, 끝나면 입력을 닫아
//! CLI가 스스로 끝나게 한 뒤 1초 안에 안 끝나면 이 조회가 만든 프로세스만 강제로 끝낸다. 앱이 끝날 때 남은 자식도 끝낸다.
//! 같은 계정 폴더의 동시 요청은 하나로 합치고, 성공한 값은 2분 동안 다시 쓴다. 실패 사유는 CLI의 원문을 담지 않는다.
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use paddock_core::codex_usage::{
    CodexEnv, CodexUsage, Conversation, Invocation, LookupFailed, Step, codex_usage_command, initialize_request,
    rate_limit_windows,
};
use paddock_core::usage_model::Reading;

use crate::app_log;

/// 조회 시간 제한. 운영 값은 `CodexUsage`의 상수다.
#[derive(Clone, Copy, Debug)]
struct Timing {
    timeout: Duration,
    cleanup: Duration,
    /// 표준 출력이 이 바이트 수를 넘으면 조회를 실패로 끝낸다(줄바꿈이 없어도 센다).
    n_output_bytes: usize,
}

impl Default for Timing {
    fn default() -> Self {
        Self { timeout: CodexUsage::TIMEOUT, cleanup: CodexUsage::CLEANUP, n_output_bytes: CodexUsage::N_OUTPUT_BYTES }
    }
}

/// 같은 계정 폴더의 동시 요청이 기다리는 결과 칸.
type Slot = Arc<(Mutex<Option<Result<Reading, LookupFailed>>>, Condvar)>;

/// 조회기: 계정 폴더별 합치기와 2분 캐시를 쥔다.
#[derive(Default)]
pub struct CodexReader {
    cached: Mutex<BTreeMap<String, (Instant, Reading)>>,
    pending: Mutex<BTreeMap<String, Slot>>,
    timing: Timing,
}

/// 지금 돌고 있는 조회의 자식 프로세스: 앱이 끝날 때 남기지 않으려고 쥔다.
static RUNNING: Mutex<Vec<(u32, bool, bool)>> = Mutex::new(Vec::new());

impl CodexReader {
    #[cfg(all(test, unix))]
    fn with_timing(
        timeout: Duration,
        cleanup: Duration,
    ) -> Self {
        Self { timing: Timing { timeout, cleanup, ..Timing::default() }, ..Self::default() }
    }

    #[cfg(all(test, unix))]
    fn with_output_limit(n_output_bytes: usize) -> Self {
        Self { timing: Timing { n_output_bytes, ..Timing::default() }, ..Self::default() }
    }

    /// 계정 폴더(`env`)의 한도를 읽는다. 2분 안의 성공 값은 그대로 쓰고, 같은 폴더를 읽는 중이면 그 결과를 기다린다.
    pub fn read(
        &self,
        env: &CodexEnv,
    ) -> Result<Reading, LookupFailed> {
        self.read_at(env, Instant::now())
    }

    fn read_at(
        &self,
        env: &CodexEnv,
        now: Instant,
    ) -> Result<Reading, LookupFailed> {
        let key = env.cache_key();
        let cached = self.cached.lock().ok().and_then(|cache| {
            cache
                .get(&key)
                .filter(|(read_at, _)| now.duration_since(*read_at).as_secs() < CodexUsage::CACHE_SECONDS)
                .map(|(_, value)| value.clone())
        });
        match cached {
            Some(value) => Ok(value),
            None => self.read_uncached(env, &key, now),
        }
    }

    /// 캐시에 없는 폴더를 읽는다: 같은 폴더를 읽는 중이면 그 결과를 기다리고, 아니면 이 호출이 CLI를 띄운다.
    fn read_uncached(
        &self,
        env: &CodexEnv,
        key: &str,
        now: Instant,
    ) -> Result<Reading, LookupFailed> {
        let (slot, leader) = {
            let mut pending = self.pending.lock().map_err(|_| LookupFailed)?;
            match pending.get(key) {
                Some(slot) => (slot.clone(), false),
                None => {
                    let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
                    pending.insert(key.to_string(), slot.clone());
                    (slot, true)
                }
            }
        };
        if leader {
            let result = run_lookup(&codex_usage_command(env, cfg!(windows)), self.timing);
            if let (Ok(value), Ok(mut cache)) = (&result, self.cached.lock()) {
                cache.insert(key.to_string(), (now, value.clone()));
            }
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(key);
            }
            if let Ok(mut filled) = slot.0.lock() {
                *filled = Some(result.clone());
            }
            slot.1.notify_all();
            result
        } else {
            let (lock, signal) = &*slot;
            let mut filled = lock.lock().map_err(|_| LookupFailed)?;
            while filled.is_none() {
                filled = signal.wait(filled).map_err(|_| LookupFailed)?;
            }
            filled.clone().unwrap_or(Err(LookupFailed))
        }
    }
}

/// 앱이 끝날 때 아직 도는 조회의 자식 프로세스를 끝낸다.
pub fn terminate_all() {
    let running: Vec<(u32, bool, bool)> = RUNNING.lock().map(|list| list.clone()).unwrap_or_default();
    for (pid, own_process_group, kill_tree) in running {
        kill_process(pid, own_process_group, kill_tree);
    }
}

/// 이 조회가 만든 프로세스(트리)만 강제로 끝낸다.
fn kill_process(
    pid: u32,
    own_process_group: bool,
    kill_tree: bool,
) {
    #[cfg(unix)]
    {
        let _ = kill_tree;
        let target = if own_process_group { -(pid as i32) } else { pid as i32 };
        // SAFETY: 이 조회가 직접 띄운 자식의 pid(또는 그 프로세스 그룹)에만 신호를 보낸다.
        unsafe {
            libc::kill(target, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = own_process_group;
        // 끝날 때까지 기다린다: 부모(PowerShell 등)를 먼저 죽이면 taskkill이 트리를 찾지 못해 손자가 남는다.
        // `/T`는 이 조회가 만든 프로세스 트리만, `kill_tree`가 아니면 그 프로세스 하나만 끝낸다.
        let mut arguments = vec!["/PID".to_string(), pid.to_string()];
        if kill_tree {
            arguments.push("/T".to_string());
        }
        arguments.push("/F".to_string());
        let _ = Command::new("taskkill.exe")
            .args(&arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .status();
    }
}

fn spawn(invocation: &Invocation) -> std::io::Result<Child> {
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(cwd) = &invocation.cwd {
        command.current_dir(cwd);
    }
    for name in &invocation.env_remove {
        command.env_remove(name);
    }
    for (name, value) in &invocation.env_set {
        command.env(name, value);
    }
    #[cfg(unix)]
    if invocation.own_process_group {
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command.spawn()
}

/// 조회 하나를 돈다: `initialize` → `initialized` + 한도 읽기 → 응답. 시간 제한·출력 상한을 넘기면 실패다.
fn run_lookup(
    invocation: &Invocation,
    timing: Timing,
) -> Result<Reading, LookupFailed> {
    let mut child = spawn(invocation).map_err(|_| {
        app_log::event("codex usage: could not start the CLI");
        LookupFailed
    })?;
    let pid = child.id();
    if let Ok(mut running) = RUNNING.lock() {
        running.push((pid, invocation.own_process_group, invocation.kill_tree));
    }
    let outcome = converse(&mut child, timing);
    // 입력을 닫으면 CLI는 보통 스스로 끝난다. 짧게 기다려 안 끝나면 이 조회의 프로세스만 강제로 끝낸다.
    drop(child.stdin.take());
    let deadline = Instant::now() + timing.cleanup;
    let mut ended = false;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !ended {
        // 먼저 트리(프로세스 그룹)를 끝내고 기다린 뒤에 직접 자식을 거둔다.
        kill_process(pid, invocation.own_process_group, invocation.kill_tree);
        let _ = child.kill();
        let _ = child.wait();
    } else if invocation.own_process_group {
        // 자식은 끝났어도 같은 프로세스 그룹에 남은 손자가 없게 한다.
        kill_process(pid, true, false);
    }
    if let Ok(mut running) = RUNNING.lock() {
        running.retain(|(running_pid, _, _)| *running_pid != pid);
    }
    outcome
}

fn converse(
    child: &mut Child,
    timing: Timing,
) -> Result<Reading, LookupFailed> {
    let mut stdin = child.stdin.take().ok_or(LookupFailed)?;
    let stdout = child.stdout.take().ok_or(LookupFailed)?;
    let (lines, received) = mpsc::channel::<Option<String>>();
    let n_limit = timing.n_output_bytes;
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut n_bytes = 0;
        loop {
            // 상한을 넘는 한 바이트까지만 읽는다: 줄바꿈 없이 큰 출력이 와도 메모리에 상한 넘게 쌓지 않는다.
            let mut line = Vec::new();
            let room = (n_limit + 1 - n_bytes) as u64;
            match (&mut reader).take(room).read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => {
                    let _ = lines.send(None);
                    break;
                }
                Ok(n_read) => {
                    n_bytes += n_read;
                    if n_bytes > n_limit {
                        let _ = lines.send(None);
                        break;
                    }
                    let text = String::from_utf8_lossy(&line).trim_end().to_string();
                    if lines.send(Some(text)).is_err() {
                        break;
                    }
                }
            }
        }
    });
    writeln!(stdin, "{}", initialize_request()).map_err(|_| LookupFailed)?;
    stdin.flush().map_err(|_| LookupFailed)?;
    let deadline = Instant::now() + timing.timeout;
    let mut conversation = Conversation::default();
    let mut result = Err(LookupFailed);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match received.recv_timeout(remaining) {
            Ok(Some(line)) => match conversation.on_line(&line) {
                Step::Wait => {}
                Step::Send(requests) => {
                    let sent = requests
                        .iter()
                        .try_for_each(|request| writeln!(stdin, "{request}"))
                        .and_then(|()| stdin.flush());
                    if sent.is_err() {
                        break;
                    }
                }
                Step::Done(Ok(response)) => {
                    result = Ok(Reading {
                        windows: rate_limit_windows(&response),
                        updated_at: Some(chrono::Utc::now().timestamp()),
                        ..Reading::default()
                    });
                    break;
                }
                Step::Done(Err(LookupFailed)) => break,
            },
            Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                app_log::event("codex usage: the CLI did not answer in time");
                break;
            }
        }
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use paddock_core::agent_account::Runtime;

    use super::*;
    use crate::test_support::TempHome;

    const LIMITS: &str = r#"{"rateLimits":{"primary":{"usedPercent":0,"windowDurationMins":300,"resetsAt":1999999999},"secondary":{"usedPercent":24.6,"windowDurationMins":10080,"resetsAt":1999999998}}}"#;

    /// 가짜 `codex`가 든 폴더를 PATH 맨 앞에 둔다(시험이 쥔 임시 홈 잠금 안에서만).
    struct FakeCodex {
        _dir: tempfile::TempDir,
        saved_path: Option<std::ffi::OsString>,
    }

    impl FakeCodex {
        fn install(script: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("codex");
            std::fs::write(&file, script).unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            let saved_path = std::env::var_os("PATH");
            let mut paths = vec![dir.path().to_path_buf()];
            paths.extend(std::env::split_paths(&saved_path.clone().unwrap_or_default()));
            // SAFETY: 환경 변수를 건드리는 시험은 `TempHome`의 잠금을 쥐고 있다.
            unsafe { std::env::set_var("PATH", std::env::join_paths(paths).unwrap()) };
            Self { _dir: dir, saved_path }
        }
    }

    impl Drop for FakeCodex {
        fn drop(&mut self) {
            // SAFETY: 잠금을 쥔 시험 안에서만 쓴다.
            unsafe {
                match &self.saved_path {
                    Some(path) => std::env::set_var("PATH", path),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }

    /// `app-server` 흉내: 줄을 읽어 `initialize`와 한도 읽기에 답하고, 받은 요청의 method를 파일에 남기고, 시작할 때 pid를 남긴다.
    fn answering(
        log: &Path,
        pid_file: &Path,
    ) -> String {
        format!(
            "#!/bin/bash\necho $$ > '{pid}'\nwhile IFS= read -r line; do\n  echo \"$line\" | sed -E 's/.*\"method\":\"([^\"]*)\".*/\\1/' >> '{log}'\n  case \"$line\" in\n    *'\"initialize\"'*) echo '{{\"id\":1,\"result\":{{}}}}';;\n    *'rateLimits/read'*) echo '{{\"id\":2,\"result\":{limits}}}';;\n  esac\ndone\n",
            pid = pid_file.display(),
            log = log.display(),
            limits = LIMITS
        )
    }

    fn env(config: &Path) -> CodexEnv {
        CodexEnv {
            runtime: Runtime::Native,
            wsl_distribution: None,
            config_dir: config.to_string_lossy().into_owned(),
            sessions_dir: config.join("sessions").to_string_lossy().into_owned(),
        }
    }

    fn alive(pid_file: &Path) -> bool {
        std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
            // SAFETY: 신호 0은 프로세스가 있는지만 묻는다.
            .is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)
    }

    fn wait_gone(pid_file: &Path) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while alive(pid_file) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        !alive(pid_file)
    }

    #[test]
    fn test_cu02_initialized_precedes_the_limit_request_and_successful_results_are_cached_by_account() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(work.join("a")).unwrap();
        std::fs::create_dir_all(work.join("b")).unwrap();
        let (log, pid_file) = (work.join("requests.log"), work.join("pid"));
        let _fake = FakeCodex::install(&answering(&log, &pid_file));
        let reader = CodexReader::default();
        let started = Instant::now();
        let first = reader.read_at(&env(&work.join("a")), started).unwrap();
        assert_eq!(first.windows[0].used, 0);
        assert_eq!(first.windows[1].used, 25);
        let methods = std::fs::read_to_string(&log).unwrap();
        assert_eq!(methods.lines().collect::<Vec<_>>(), vec!["initialize", "initialized", "account/rateLimits/read"]);
        assert!(wait_gone(&pid_file), "the child ends once its input is closed");
        std::fs::remove_file(&log).unwrap();
        let again = reader.read_at(&env(&work.join("a")), started + Duration::from_secs(60)).unwrap();
        assert_eq!(again, first);
        assert!(!log.exists(), "a result inside two minutes is reused without a new CLI");
        reader.read_at(&env(&work.join("b")), started + Duration::from_secs(60)).unwrap();
        assert!(log.exists(), "another account folder is read on its own");
    }

    #[test]
    fn test_cu06_two_minute_expiry_refreshes_the_same_account_without_keeping_an_earlier_response_indefinitely() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let (log, pid_file) = (work.join("requests.log"), work.join("pid"));
        let _fake = FakeCodex::install(&answering(&log, &pid_file));
        let reader = CodexReader::default();
        let start = Instant::now();
        reader.read_at(&env(&work), start).unwrap();
        std::fs::remove_file(&log).unwrap();
        reader.read_at(&env(&work), start + Duration::from_secs(119)).unwrap();
        assert!(!log.exists());
        reader.read_at(&env(&work), start + Duration::from_secs(120)).unwrap();
        assert!(log.exists(), "after two minutes the CLI is asked again");
    }

    #[test]
    fn test_cu04_official_server_errors_never_expose_raw_cli_output() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let _fake = FakeCodex::install(
            "#!/bin/bash\nwhile IFS= read -r line; do\n  echo '{\"id\":1,\"error\":{\"code\":-32000,\"message\":\"private-value-must-not-escape\"}}'\ndone\n",
        );
        let reader = CodexReader::default();
        let result = reader.read(&env(&work));
        assert_eq!(result, Err(LookupFailed));
        let log = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs/app.log"))
            .unwrap_or_default();
        assert!(!log.contains("private-value"), "{log}");
    }

    #[test]
    fn test_c_d74_codex_live_lookup_times_out_and_leaves_no_child() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let pid_file = work.join("pid");
        let silent = format!("#!/bin/bash\necho $$ > '{}'\nsleep 60\n", pid_file.display());
        let _fake = FakeCodex::install(&silent);
        // 제한 시간은 가짜 CLI가 뜰 만큼은 줘야 한다. macOS는 대화형 bash(`bash -ic`)가 뜨는 데만 0.5초 넘게 걸려
        // 400ms로는 가짜 CLI가 pid를 쓰기 전에 끝나 버렸다. 2초여도 60초 sleep을 기다리지 않는다는 판정은 그대로다.
        let reader = CodexReader::with_timing(Duration::from_millis(2000), Duration::from_millis(200));
        let started = Instant::now();
        assert_eq!(reader.read(&env(&work)), Err(LookupFailed));
        assert!(started.elapsed() < Duration::from_secs(5), "it did not wait for the sleep");
        assert!(pid_file.exists(), "the fake CLI did start");
        assert!(wait_gone(&pid_file), "an unresponsive lookup is reaped and no child stays");
        assert!(RUNNING.lock().unwrap().is_empty(), "the finished lookup is no longer listed as running");
    }

    #[test]
    fn test_cu_concurrent_requests_for_one_folder_share_one_child() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let (log, pid_file) = (work.join("requests.log"), work.join("pid"));
        let _fake = FakeCodex::install(&answering(&log, &pid_file));
        let reader = Arc::new(CodexReader::default());
        let folder = env(&work);
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let (reader, folder) = (reader.clone(), folder.clone());
                std::thread::spawn(move || reader.read(&folder))
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
        assert!(results.iter().all(|result| result == &results[0] && result.is_ok()));
        let n_asks = std::fs::read_to_string(&log).unwrap().lines().filter(|line| *line == "initialize").count();
        assert_eq!(n_asks, 1, "three readers asked the CLI once");
    }

    #[test]
    fn test_c_d74_the_lookup_environment_drops_authentication_selectors() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let seen = work.join("seen");
        let script = format!(
            "#!/bin/bash\n{{ echo \"home=$CODEX_HOME\"; echo \"key=${{OPENAI_API_KEY-unset}}\"; }} > '{}'\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"initialize\"'*) echo '{{\"id\":1,\"result\":{{}}}}';;\n    *'rateLimits/read'*) echo '{{\"id\":2,\"result\":{}}}';;\n  esac\ndone\n",
            seen.display(),
            LIMITS
        );
        let _fake = FakeCodex::install(&script);
        // SAFETY: TempHome이 환경 잠금을 쥐고 있다.
        unsafe { std::env::set_var("OPENAI_API_KEY", "synthetic-selector-value") };
        let result = CodexReader::default().read(&env(&work));
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        assert!(result.is_ok());
        let text = std::fs::read_to_string(seen).unwrap();
        assert!(text.contains(&format!("home={}", work.display())), "{text}");
        assert!(text.contains("key=unset"), "{text}");
    }
    #[test]
    fn test_c_d74_a_grandchild_that_outlives_the_cli_is_ended_with_the_lookup() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let (pids, held) = (work.join("grandchild.pid"), work.join("held.pid"));
        // 손자(`sleep`)를 띄우고 답한 뒤: 하나는 손자를 기다리며 입력이 닫혀도 끝나지 않고, 하나는 손자를 두고 끝난다.
        for (name, tail) in [("waits", "wait"), ("leaves", "")] {
            let target = if name == "waits" { &held } else { &pids };
            let script = format!(
                "#!/bin/bash\nsleep 300 &\necho $! > '{}'\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *'\"initialize\"'*) echo '{{\"id\":1,\"result\":{{}}}}';;\n    *'rateLimits/read'*) echo '{{\"id\":2,\"result\":{}}}';;\n  esac\ndone\n{tail}\n",
                target.display(),
                LIMITS
            );
            let _fake = FakeCodex::install(&script);
            let reader = CodexReader::with_timing(Duration::from_secs(5), Duration::from_millis(300));
            assert!(reader.read(&env(&work)).is_ok(), "{name}");
            assert!(wait_gone(target), "the grandchild of the '{name}' CLI is gone after the lookup");
        }
    }

    #[test]
    fn test_cu_output_without_a_newline_is_cut_at_the_byte_limit() {
        let home = TempHome::new();
        let work = home.dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        // 줄바꿈 없이 상한(2 KiB)을 크게 넘는 출력을 쏟는다. 답이 오기 전에 상한에서 실패로 끝나야 한다.
        let _fake = FakeCodex::install("#!/bin/bash\nhead -c 1000000 /dev/zero | tr '\\0' 'x'\nsleep 20\n");
        let reader = CodexReader::with_output_limit(2048);
        let started = Instant::now();
        assert_eq!(reader.read(&env(&work)), Err(LookupFailed));
        assert!(started.elapsed() < Duration::from_secs(10), "it did not wait for the 15-second limit");
    }
}
