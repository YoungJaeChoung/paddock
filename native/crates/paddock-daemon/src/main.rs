//! 터미널 데몬: 터미널마다 PTY와 원시 출력 링 버퍼를 쥐고, 앱이 붙으면 링을 재생한 뒤 실시간 출력을 보낸다.
//! 화면 격자는 만들지 않는다. 터미널이 0개이고 연결도 0개인 채로 10초가 지나면 스스로 끝난다.
//!
//! `--breakaway-denied`: 앱이 데몬을 부모 작업 개체에서 분리하려다 거부당했음을 알리는 실행 인자.
//! 데몬이 `daemon.json`에 그대로 기록한다(접속 정보를 쓰는 주체를 데몬 하나로 두기 위해서다).
mod activity;
mod history;
mod log;
mod outbox;
mod probe;
mod pty;
mod server;
mod terminal;
#[cfg(test)]
mod test_home;

use std::collections::BTreeMap;
use std::io;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt as _;
use paddock_core::shell_catalog::clear_inherited_session;
use paddock_protocol::daemon_info::{DaemonInfo, ensure_private_dir, native_dir, new_token, write_info};
use paddock_protocol::daemon_lock::DaemonLock;
use paddock_protocol::endpoint::{create_listener, endpoint_name};
use paddock_protocol::fingerprint::fingerprint_of;
use paddock_protocol::peer::current_user;

use crate::activity::{TICK_INTERVAL, now_ms, tick};
use crate::history::{History, is_enabled};
use crate::probe::SystemProbe;
use crate::server::{Context, exit_daemon, serve};
use crate::terminal::Registry;

/// 터미널이 0개이고 연결도 0개인 채로 이 시간이 지나면 데몬이 끝난다.
const IDLE_EXIT: Duration = Duration::from_secs(10);

/// 단일 실행 잠금을 얻으려고 기다리는 최대 시간. 교체되는 옛 데몬이 끝나기를 기다린다.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// 잠금을 다시 시도하는 간격.
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// 유휴 시간을 확인하는 간격.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_millis(500);

fn main() -> ExitCode {
    install_panic_log();
    clear_inherited_session_markers();
    let breakaway_denied = std::env::args().any(|arg| arg == "--breakaway-denied");
    match run(breakaway_denied) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::event(&format!("daemon could not start: {error}"));
            ExitCode::from(2)
        }
    }
}

/// 스레드가 panic하면 어느 스레드가 어느 위치에서 멈췄는지 한 줄만 기록 파일에 남긴다.
///
/// panic 메시지는 쓰지 않는다 — 메시지에 처리 중이던 출력·경로 조각이 섞일 수 있고, 기록 파일은 입력·출력·토큰을 담지 않는다.
fn install_panic_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place =
            info.location().map_or_else(String::new, |location| format!(" at {}:{}", location.file(), location.line()));
        log::event(&format!("thread '{}' panicked{place}", thread.name().unwrap_or("unnamed")));
        previous(info);
    }));
}

/// 단일 실행 잠금을 얻은 뒤 연결을 받기 시작한다. 시작에 실패하면 그 오류를 돌려준다.
fn run(breakaway_denied: bool) -> io::Result<()> {
    ensure_private_dir(&native_dir())?;
    match acquire_lock()? {
        Some(_lock) => listen(breakaway_denied),
        None => Err(io::Error::new(io::ErrorKind::AddrInUse, "another daemon is already running")),
    }
}

/// 단일 실행 잠금을 얻는다. 교체되는 옛 데몬이 막 끝나는 중일 수 있어 `LOCK_WAIT` 동안 다시 시도하고,
/// 그래도 다른 데몬이 쥐고 있으면 `None`이다.
fn acquire_lock() -> io::Result<Option<DaemonLock>> {
    let end = Instant::now() + LOCK_WAIT;
    let mut lock = DaemonLock::try_acquire()?;
    while lock.is_none() && Instant::now() < end {
        std::thread::sleep(LOCK_RETRY_INTERVAL);
        lock = DaemonLock::try_acquire()?;
    }
    Ok(lock)
}

/// 리스너를 열고 접속 정보를 쓴 뒤 연결을 받는다. 호출자가 단일 실행 잠금을 쥔 채여야 한다 —
/// 잠금이 있어야 이전 데몬이 남긴 소켓 파일을 덮어써도 살아 있는 데몬의 것이 아님이 보장된다.
fn listen(breakaway_denied: bool) -> io::Result<()> {
    let endpoint = endpoint_name()?;
    let listener = create_listener(&endpoint)?;
    let token = new_token();
    let started = chrono::Local::now().to_rfc3339();
    let fingerprint = fingerprint_of(&std::env::current_exe()?)?;
    write_info(&DaemonInfo {
        pid: std::process::id(),
        endpoint: endpoint.clone(),
        token: token.clone(),
        fingerprint: fingerprint.clone(),
        started: started.clone(),
        breakaway_denied,
    })?;
    log::event(&format!("daemon started (pid {}, breakaway denied: {breakaway_denied})", std::process::id()));
    #[cfg(windows)]
    log_windows_environment(&endpoint);

    let history = Arc::new(History::new(&format!("{}@{}", std::process::id(), started)));
    // 되살리기가 꺼져 있으면 이전 기록을 모두 지우고, 켜져 있으면 14일 넘은 다른 데몬의 기록만 지운다.
    if is_enabled() {
        let n_removed = history.prune_old();
        if n_removed > 0 {
            log::event(&format!("terminal history: removed {n_removed} old folders"));
        }
    } else {
        history.clear_all();
    }
    let ctx = Arc::new(Context {
        registry: Arc::new(Registry::new(history)),
        token,
        fingerprint,
        own_user: current_user()?,
        endpoint,
    });
    watch_idle(ctx.clone());
    watch_terminals(ctx.registry.clone());
    watch_history(ctx.registry.clone());

    // 받는 데 실패한 연결은 건너뛴다.
    for (index, stream) in listener.incoming().flatten().enumerate() {
        let ctx = ctx.clone();
        let conn_id = index as u64 + 1;
        std::thread::spawn(move || serve(stream, conn_id, &ctx));
    }
    Ok(())
}

/// 상주 스레드를 띄워 `interval`마다 `round`를 부른다. `round`가 panic해도 스레드는 그 판만 버리고 다음 판을 이어 간다.
///
/// 상주 스레드가 panic으로 조용히 죽으면 데몬은 살아 있는데 터미널 상태 갱신·기록 저장·유휴 종료가 멈춘다.
/// 한 판의 실패가 다음 판에 영향을 주지 않는 반복 작업이므로 데몬을 끝내지 않고 다음 판에서 다시 시도한다.
/// panic 위치는 panic 훅이 따로 기록하고, 여기서는 이 스레드가 이어 간다는 한 줄을 남긴다.
fn spawn_watcher(
    name: &'static str,
    interval: Duration,
    mut round: impl FnMut() + Send + 'static,
) {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(interval);
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut round)).is_err() {
                    log::event(&format!("{name} stopped after a panic, running again"));
                }
            }
        })
        .expect("spawn watcher thread");
}

/// 터미널과 연결이 모두 0개인 채로 `IDLE_EXIT`가 지나면 접속 정보를 지우고 끝내는 스레드를 띄운다.
fn watch_idle(ctx: Arc<Context>) {
    spawn_watcher("idle watcher", IDLE_CHECK_INTERVAL, move || {
        ctx.registry.when_idle_for(IDLE_EXIT, || {
            log::event("idle: no terminals or connections, exiting");
            exit_daemon(&ctx)
        });
    });
}

/// 이 데몬을 띄운 Claude Code 세션의 표지를 환경에서 지운다. 지우지 않으면 데몬이 띄우는 모든 셸이 그 표지를 물려받는다.
fn clear_inherited_session_markers() {
    let mut env: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    for name in clear_inherited_session(&mut env) {
        // SAFETY: 스레드를 띄우기 전에 `main`이 맨 처음 한 번만 부른다.
        unsafe { std::env::remove_var(name) };
    }
}

/// 모든 터미널의 폴더·앞쪽 프로그램·에이전트 활동을 `TICK_INTERVAL`마다 판정하는 스레드를 띄운다. 스레드는 데몬에 하나다.
fn watch_terminals(registry: Arc<Registry>) {
    let probe = SystemProbe;
    spawn_watcher("terminal watcher", TICK_INTERVAL, move || tick(&registry, &probe, now_ms()));
}

/// 터미널 기록이 저장 시점에 이르렀는지 `history::CHECK_INTERVAL`마다 보고 쓰는 스레드를 띄운다. 스레드는 데몬에 하나다.
fn watch_history(registry: Arc<Registry>) {
    spawn_watcher("history watcher", history::CHECK_INTERVAL, move || {
        registry.save_due_histories(now_ms(), &SystemProbe);
    });
}

/// 작업 개체 소속과 파이프에 건 접근 권한을 기록한다. 데몬이 앱과 함께 끝나는 문제와 접근 권한 문제를 진단하는 자료다.
///
/// 접근 권한은 파이프를 다시 열어 읽지 않고 만들 때 쓴 값을 적는다 — 열면 연결 하나를 소비해 "no hello" 기록이 남는다.
/// 실제 파이프의 권한은 `probe dacl`과 엔드포인트 시험이 확인한다.
#[cfg(windows)]
fn log_windows_environment(endpoint: &str) {
    use paddock_protocol::endpoint::pipe_sddl;
    use paddock_protocol::windows_security::{in_job, job_limit_flags};
    log::event(&format!(
        "in job: {}, job limit flags: {:?} (0x2000 kill on close, 0x800 breakaway ok, 0x1000 silent breakaway), endpoint {endpoint}",
        in_job(),
        job_limit_flags().map(|flags| format!("{flags:#x}"))
    ));
    match pipe_sddl() {
        Ok(sddl) => log::event(&format!("pipe SDDL (as created) {sddl}")),
        Err(error) => log::event(&format!("pipe SDDL unavailable: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::test_home::TempHome;

    #[test]
    fn test_c_d46_a_panicking_watcher_keeps_running_on_the_next_round() {
        let _home = TempHome::new();
        // 기록 폴더 준비는 프로세스에서 한 번만 하므로, 다른 시험이 먼저 기록을 남겼으면 이 임시 홈에는 폴더가 없다.
        std::fs::create_dir_all(native_dir().join("logs")).unwrap();
        let n_rounds = Arc::new(AtomicUsize::new(0));
        let counted = n_rounds.clone();
        spawn_watcher("test watcher", Duration::from_millis(10), move || {
            // 첫 판은 panic하고, 그 뒤 판이 계속 도는지 본다.
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("first round fails");
            }
        });
        let end = Instant::now() + Duration::from_secs(5);
        while n_rounds.load(Ordering::SeqCst) < 3 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(n_rounds.load(Ordering::SeqCst) >= 3, "the watcher thread survives a panic and runs again");
        let log = std::fs::read_to_string(native_dir().join("logs").join("daemon.log")).expect("log file");
        assert!(log.contains("test watcher stopped after a panic, running again"), "{log}");
    }
}
