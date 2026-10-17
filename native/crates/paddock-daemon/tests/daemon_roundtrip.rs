//! 데몬 실행 파일을 임시 홈 폴더로 띄워 프레임을 주고받는 통합 시험.
//!
//! 사용자의 실제 `~/.paddock/native`를 건드리지 않도록 자식 프로세스에만 `HOME`/`USERPROFILE`을 넘긴다.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use interprocess::local_socket::SendHalf;
use interprocess::local_socket::traits::Stream as _;
#[cfg(unix)]
use paddock_core::account_launch::{AUTH_ENV, AccountLaunchInput, Location, Permissions, account_terminal_spec};
#[cfg(unix)]
use paddock_core::agent_account::{AccountProfile, Runtime};
use paddock_core::terminal_history::{HistoryRecord, parse_record};
use paddock_protocol::daemon_info::DaemonInfo;
use paddock_protocol::endpoint;
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{
    Attach, Attached, Hello, HelloOk, Kill, Launch, List, New, PROTOCOL_VERSION, Status, TerminalState, kind,
};

const WAIT: Duration = Duration::from_secs(15);

/// 큰 출력이 터미널을 모두 지나가기를 기다리는 최대 시간.
const FLOOD_WAIT: Duration = Duration::from_secs(120);

/// Windows의 파이프 이름은 사용자 SID에서만 정해져 데몬이 하나만 뜰 수 있으므로 시험을 한 번에 하나씩 돌린다.
#[cfg(windows)]
static ONE_DAEMON_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
const SHELL: &str = "bash";
#[cfg(windows)]
const SHELL: &str = "cmd";

/// 끝 표지: 입력한 명령줄에는 나타나지 않고 실행 결과로만 나타나는 글자.
#[cfg(unix)]
const ECHO_MARKER: &str = "echo MARK''ER\n";
#[cfg(windows)]
const ECHO_MARKER: &str = "echo MARK^ER\r\n";
const MARKER: &str = "MARKER";

/// 시험 대상 데몬 실행 파일. 시험 실행 파일 옆에 복사된 것이 있으면 그것을 쓴다.
///
/// 교차 빌드한 시험 파일을 다른 기계에서 돌릴 때 빌드 시점의 절대 경로가 맞지 않기 때문이다.
fn daemon_exe() -> PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(format!("paddock-daemon{}", std::env::consts::EXE_SUFFIX))));
    match beside {
        Some(path) if path.exists() => path,
        _ => PathBuf::from(env!("CARGO_BIN_EXE_paddock-daemon")),
    }
}

/// 사용자 데몬이 이미 이 사용자의 파이프를 쥐고 있으면 시험 데몬이 같은 이름을 만들지 못한다.
/// 직전 시험의 데몬이 막 끝나는 중일 수 있어 잠시 기다린 뒤에도 남아 있으면 무엇이 떠 있는지 밝히며 멈춘다.
///
/// 파이프를 열어 보지 않고 파이프 목록에서 이름만 찾는다 — 열면 떠 있는 데몬에 연결 하나를 거는 셈이다.
#[cfg(windows)]
fn assert_no_daemon_pipe_is_taken() {
    let endpoint = endpoint::endpoint_name().expect("endpoint name");
    let pipe_name = endpoint.rsplit('\\').next().expect("pipe name").to_string();
    let is_taken = || {
        std::fs::read_dir(r"\\.\pipe\")
            .expect("list pipes")
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy() == pipe_name.as_str())
    };
    let end = Instant::now() + Duration::from_secs(3);
    while is_taken() && Instant::now() < end {
        thread::sleep(Duration::from_millis(100));
    }
    if is_taken() {
        let running = match paddock_protocol::daemon_info::read_info() {
            Some(info) => format!("pid {} started {}", info.pid, info.started),
            None => "no readable daemon.json in the real profile".to_string(),
        };
        panic!(
            "Cannot run the daemon tests: pipe {endpoint} is already served by a running Paddock daemon ({running}). \
             Stop it first (it exits 10 s after its last terminal and connection close); the tests start their own daemons under a temporary profile."
        );
    }
}

struct TestDaemon {
    child: Child,
    home: tempfile::TempDir,
    #[cfg(windows)]
    _one_at_a_time: std::sync::MutexGuard<'static, ()>,
}

impl TestDaemon {
    fn start() -> Self {
        Self::start_with(&[])
    }

    fn start_with(args: &[&str]) -> Self {
        let daemon = Self::spawn_without_waiting(args);
        daemon.wait_for_info();
        daemon
    }

    #[cfg(unix)]
    /// 데몬 환경에 `env`를 더해 띄운다(로그인 값이 데몬 환경에 있을 때 계정 터미널이 지우는지 보려는 시험용).
    fn start_with_env(env: &[(&str, &str)]) -> Self {
        let daemon = Self::spawn_with_env(&[], env);
        daemon.wait_for_info();
        daemon
    }

    /// 데몬을 띄우기 전에 `prepare`가 임시 홈 폴더를 채우게 한다(미리 있던 설정·기록 파일이 필요한 시험용).
    fn start_after(prepare: impl FnOnce(&Path)) -> Self {
        #[cfg(windows)]
        let guard = ONE_DAEMON_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        #[cfg(windows)]
        assert_no_daemon_pipe_is_taken();
        let home = tempfile::tempdir().expect("temp home");
        prepare(home.path());
        let child = Self::spawn_in(home.path(), &[], &[]);
        let daemon = Self {
            child,
            home,
            #[cfg(windows)]
            _one_at_a_time: guard,
        };
        daemon.wait_for_info();
        daemon
    }

    fn spawn_without_waiting(args: &[&str]) -> Self {
        Self::spawn_with_env(args, &[])
    }

    fn spawn_with_env(
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
        #[cfg(windows)]
        let guard = ONE_DAEMON_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        #[cfg(windows)]
        assert_no_daemon_pipe_is_taken();
        let home = tempfile::tempdir().expect("temp home");
        let child = Self::spawn_in(home.path(), args, env);
        Self {
            child,
            home,
            #[cfg(windows)]
            _one_at_a_time: guard,
        }
    }

    fn spawn_in(
        home: &Path,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Child {
        Command::new(daemon_exe())
            .args(args)
            .envs(env.iter().copied())
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env_remove("XDG_RUNTIME_DIR")
            // 시험을 돌리는 사용자의 계정 폴더 설정이 데몬이 띄우는 터미널로 새지 않게 한다.
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start daemon")
    }

    /// 같은 홈 폴더를 쓰는 데몬을 하나 더 띄운다.
    fn spawn_rival(&self) -> Child {
        Self::spawn_in(self.home.path(), &[], &[])
    }

    fn info_path(&self) -> PathBuf {
        self.home.path().join(".paddock").join("native").join("daemon.json")
    }

    fn log_path(&self) -> PathBuf {
        self.home.path().join(".paddock").join("native").join("logs").join("daemon.log")
    }

    fn read_info(&self) -> Option<DaemonInfo> {
        serde_json::from_slice(&std::fs::read(self.info_path()).ok()?).ok()
    }

    fn wait_for_info(&self) -> DaemonInfo {
        let end = Instant::now() + WAIT;
        let mut found = None;
        while found.is_none() && Instant::now() < end {
            found = self.read_info();
            if found.is_none() {
                thread::sleep(Duration::from_millis(30));
            }
        }
        found.expect("daemon.json was not written")
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn is_running(&mut self) -> bool {
        self.child.try_wait().expect("poll daemon").is_none()
    }

    fn wait_for_exit(
        &mut self,
        limit: Duration,
    ) -> bool {
        let end = Instant::now() + limit;
        let mut exited = false;
        while !exited && Instant::now() < end {
            exited = !self.is_running();
            if !exited {
                thread::sleep(Duration::from_millis(100));
            }
        }
        exited
    }

    fn connect_authenticated(&self) -> (Client, HelloOk) {
        let info = self.wait_for_info();
        let mut client = Client::connect(&info.endpoint);
        client.send(Frame::json(kind::HELLO, &Hello { token: info.token, protocol_version: PROTOCOL_VERSION }));
        let reply = client.expect(kind::HELLO_OK);
        let hello_ok = reply.parse_json().expect("hello_ok json");
        (client, hello_ok)
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Client {
    send: SendHalf,
    rx: Receiver<Frame>,
    /// 참이면 읽기 스레드가 다음 프레임을 읽지 않는다(소켓을 읽지 않는 느린 클라이언트).
    paused: Arc<AtomicBool>,
    /// 읽기 스레드를 멈추는 신호. 연결을 끊으려면 이것을 켜고 보내는 쪽을 버린다.
    stop: Arc<AtomicBool>,
}

impl Client {
    fn connect(endpoint: &str) -> Self {
        let stream = endpoint::connect(endpoint).expect("connect to daemon");
        let (mut recv, send) = stream.split();
        let (tx, rx) = channel();
        let paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_paused, thread_stop) = (paused.clone(), stop.clone());
        thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                if thread_paused.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(10));
                } else if let Ok(frame) = Frame::read_from(&mut recv) {
                    if tx.send(frame).is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
        Self { send, rx, paused, stop }
    }

    /// 소켓 읽기를 멈추거나 다시 시작한다. 멈추기 전에 읽던 프레임 하나는 마저 읽는다.
    fn set_paused(
        &self,
        paused: bool,
    ) {
        self.paused.store(paused, Ordering::SeqCst);
    }

    fn send(
        &mut self,
        frame: Frame,
    ) {
        frame.write_to(&mut self.send).expect("send frame");
    }

    /// 다음 프레임. 연결이 끊겼거나 시간이 지나면 None.
    fn next(
        &self,
        limit: Duration,
    ) -> Option<Frame> {
        self.rx.recv_timeout(limit).ok()
    }

    /// 지정한 종류가 올 때까지 다른 프레임은 건너뛴다.
    fn expect(
        &self,
        wanted: u8,
    ) -> Frame {
        let end = Instant::now() + WAIT;
        let mut found = None;
        while found.is_none() {
            let left = end
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting for kind {wanted}"));
            let frame = self.next(left).unwrap_or_else(|| panic!("connection ended waiting for kind {wanted}"));
            if frame.kind == wanted {
                found = Some(frame);
            }
        }
        found.unwrap()
    }

    fn new_terminal(
        &mut self,
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Attached {
        let request = New {
            shell: SHELL.into(),
            cwd: cwd.map(str::to_string),
            cols,
            rows,
            wsl_distribution: None,
            launch: None,
            preload: None,
            env: Vec::new(),
        };
        self.send(Frame::json(kind::NEW, &request));
        self.expect(kind::ATTACHED).parse_json().expect("attached json")
    }

    /// 출력(재생 포함)을 `needle`이 나올 때까지 모아 돌려준다.
    fn collect_until(
        &self,
        needle: &str,
    ) -> Vec<u8> {
        let end = Instant::now() + WAIT;
        let mut bytes = Vec::new();
        while !String::from_utf8_lossy(&bytes).contains(needle) {
            let left = end.checked_duration_since(Instant::now()).unwrap_or_else(|| {
                panic!("timed out waiting for {needle:?}; got {:?}", String::from_utf8_lossy(&bytes))
            });
            let frame = self.next(left).unwrap_or_else(|| panic!("connection ended waiting for {needle:?}"));
            if frame.kind == kind::REPLAY || frame.kind == kind::OUTPUT {
                bytes.extend_from_slice(&frame.payload);
            }
        }
        bytes
    }

    /// 출력이 `quiet` 동안 멎을 때까지 모아 돌려준다.
    fn collect_until_quiet(
        &self,
        mut bytes: Vec<u8>,
        quiet: Duration,
    ) -> Vec<u8> {
        while let Some(frame) = self.next(quiet) {
            if frame.kind == kind::REPLAY || frame.kind == kind::OUTPUT {
                bytes.extend_from_slice(&frame.payload);
            }
        }
        bytes
    }

    /// 데몬이 이 연결을 닫을 때까지 기다린다(읽기 스레드가 끊김으로 끝나 채널이 닫힌다). 닫히기 전에 온 프레임은 건너뛴다.
    fn wait_until_closed(
        &self,
        limit: Duration,
    ) -> bool {
        let end = Instant::now() + limit;
        let mut closed = false;
        let mut timed_out = false;
        while !closed && !timed_out {
            match self.rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => closed = true,
                Err(RecvTimeoutError::Timeout) => timed_out = true,
            }
        }
        closed
    }

    /// 연결을 끊는다. 읽기 스레드가 소켓을 쥐고 있어, 읽기를 멈추게 하고 깨운 뒤 보내는 쪽까지 버려야 닫힌다.
    fn disconnect(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = Frame::new(kind::LIST, Vec::new()).write_to(&mut self.send);
    }

    /// `matches`를 만족하는 상태 알림이 올 때까지 기다려 돌려준다. 그 전의 알림도 `seen`에 모은다.
    fn wait_for_state(
        &self,
        limit: Duration,
        seen: &mut Vec<TerminalState>,
        matches: impl Fn(&TerminalState) -> bool,
    ) -> Option<TerminalState> {
        let end = Instant::now() + limit;
        let mut found = None;
        while found.is_none() {
            let Some(left) = end.checked_duration_since(Instant::now()) else { break };
            let Some(frame) = self.next(left) else { break };
            if frame.kind == kind::TERMINAL_STATE {
                let state: TerminalState = frame.parse_json().expect("state json");
                seen.push(state.clone());
                if matches(&state) {
                    found = Some(state);
                }
            }
        }
        found
    }

    /// `duration` 동안 오는 상태 알림을 모두 모은다.
    fn collect_states(
        &self,
        duration: Duration,
    ) -> Vec<TerminalState> {
        let mut seen = Vec::new();
        self.wait_for_state(duration, &mut seen, |_| false);
        seen
    }

    fn list(&mut self) -> List {
        self.send(Frame::new(kind::LIST, Vec::new()));
        self.expect(kind::LIST_REPLY).parse_json().expect("list json")
    }
}

struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

type Grid = Term<VoidListener>;

fn grid_from(
    bytes: &[u8],
    cols: usize,
    rows: usize,
) -> Grid {
    let mut term = Term::new(Config::default(), &Size { cols, rows }, VoidListener);
    Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default().advance(&mut term, bytes);
    term
}

fn assert_same_cells(
    left: &Grid,
    right: &Grid,
    cols: usize,
    rows: usize,
) {
    assert_eq!(left.grid().cursor.point, right.grid().cursor.point, "cursor position differs");
    for row in 0..rows {
        for col in 0..cols {
            let point = Point::new(Line(row as i32), Column(col));
            let (a, b) = (&left.grid()[point], &right.grid()[point]);
            assert!(
                a.c == b.c && a.fg == b.fg && a.bg == b.bg && a.flags == b.flags,
                "cell ({row}, {col}) differs: {a:?} vs {b:?}"
            );
        }
    }
}

fn type_line(
    client: &mut Client,
    line: &str,
) {
    client.send(Frame::new(kind::INPUT, line.as_bytes().to_vec()));
}

#[test]
fn test_c_t1_bad_token_dropped_after_hello() {
    let daemon = TestDaemon::start();
    let info = daemon.wait_for_info();

    let mut wrong = Client::connect(&info.endpoint);
    wrong.send(Frame::json(kind::HELLO, &Hello { token: "wrong".into(), protocol_version: PROTOCOL_VERSION }));
    let reply = wrong.next(WAIT).expect("an error frame");
    assert_eq!(reply.kind, kind::ERR);
    assert!(!reply.as_text().is_empty());
    assert!(wrong.next(WAIT).is_none(), "connection must be closed after the error");

    // 인사가 아닌 첫 프레임은 답 없이 끊긴다.
    let mut silent = Client::connect(&info.endpoint);
    silent.send(Frame::new(kind::LIST, Vec::new()));
    assert!(silent.next(WAIT).is_none());

    // 맞는 토큰은 그대로 통하고, 데몬의 pid와 지문을 돌려받는다.
    let (_good, hello_ok) = daemon.connect_authenticated();
    assert_eq!(hello_ok.pid, daemon.pid());
    assert_eq!(hello_ok.fingerprint, info.fingerprint);
}

#[test]
fn test_c_t1_old_protocol_version_is_refused() {
    let daemon = TestDaemon::start();
    let info = daemon.wait_for_info();
    let mut client = Client::connect(&info.endpoint);
    client.send(Frame::json(kind::HELLO, &Hello { token: info.token, protocol_version: PROTOCOL_VERSION + 1 }));
    assert_eq!(client.next(WAIT).expect("an error frame").kind, kind::ERR);
    assert!(client.next(WAIT).is_none());
}

#[test]
fn test_c_r1_shell_args_not_shell_interpreted() {
    let daemon = TestDaemon::start();
    let work = tempfile::tempdir().expect("work dir");
    // 폴더 이름에 공백, 따옴표(Windows 경로에는 쓸 수 없다), 명령 치환을 넣는다.
    #[cfg(unix)]
    let awkward = "my dir \"quoted\" $(touch pwned) `touch pwned2` 'x'";
    #[cfg(windows)]
    let awkward = "my dir $(touch pwned) ; 'x'";
    let cwd = work.path().join(awkward);
    std::fs::create_dir(&cwd).expect("awkward dir");

    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(Some(cwd.to_str().unwrap()), 400, 24);
    #[cfg(unix)]
    type_line(&mut client, "pwd\n");
    #[cfg(windows)]
    type_line(&mut client, "cd\r\n");
    let output = client.collect_until(awkward);
    assert!(String::from_utf8_lossy(&output).contains(awkward));
    assert!(!cwd.join("pwned").exists());
    assert!(!work.path().join("pwned").exists());
    assert!(!cwd.join("pwned2").exists());
}

#[test]
fn test_c1_spawn_failure_reports_message() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let first = client.new_terminal(None, 80, 24);

    client.send(Frame::json(
        kind::NEW,
        &New {
            shell: "paddock-no-such-shell".into(),
            cwd: None,
            cols: 80,
            rows: 24,
            wsl_distribution: None,
            launch: None,
            preload: None,
            env: Vec::new(),
        },
    ));
    let error = client.expect(kind::ERR);
    assert!(!error.as_text().is_empty());

    // 앞서 만든 터미널은 그대로이고 연결도 계속 쓸 수 있다.
    let list = client.list();
    assert_eq!(list.terminals.len(), 1);
    assert_eq!(list.terminals[0].id, first.id);
    assert!(list.terminals[0].alive);

    // 없는 폴더도 터미널을 띄우지 못하는 실패다.
    client.send(Frame::json(
        kind::NEW,
        &New {
            shell: SHELL.into(),
            cwd: Some("/paddock/no/such/dir".into()),
            cols: 80,
            rows: 24,
            wsl_distribution: None,
            launch: None,
            preload: None,
            env: Vec::new(),
        },
    ));
    assert!(!client.expect(kind::ERR).as_text().is_empty());
    assert_eq!(client.list().terminals.len(), 1);
}

#[test]
fn test_kill_removes_only_that_terminal() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let first = client.new_terminal(None, 80, 24);
    let second = client.new_terminal(None, 80, 24);
    assert_ne!(first.id, second.id);
    assert_eq!(client.list().terminals.len(), 2);

    client.send(Frame::json(kind::KILL, &Kill { id: first.id }));
    let remaining = client.expect(kind::LIST_REPLY).parse_json::<List>().unwrap();
    assert_eq!(remaining.terminals.iter().map(|t| t.id).collect::<Vec<_>>(), vec![second.id]);
}

#[test]
fn test_c_d3b_replay_restores_grid() {
    let (cols, rows) = (100, 30);
    let daemon = TestDaemon::start();
    let (mut live, _) = daemon.connect_authenticated();
    let attached = live.new_terminal(None, cols as u16, rows as u16);
    #[cfg(unix)]
    type_line(&mut live, &format!("printf '\\033[31m한글 red\\033[0m \\033[1;44mbold\\033[0m\\n'; {ECHO_MARKER}"));
    #[cfg(windows)]
    type_line(&mut live, &format!("echo 한글 plain\r\n{ECHO_MARKER}"));
    let seen = live.collect_until(MARKER);
    let live_bytes = live.collect_until_quiet(seen, Duration::from_millis(700));

    // 두 번째 연결이 같은 터미널에 붙어 링을 재생으로 받는다.
    let mut again = daemon.connect_authenticated().0;
    again.send(Frame::json(kind::ATTACH, &Attach { id: attached.id, cols: 0, rows: 0 }));
    let reattached: Attached = again.expect(kind::ATTACHED).parse_json().unwrap();
    assert_eq!((reattached.pid, reattached.cols, reattached.rows), (attached.pid, cols as u16, rows as u16));
    let mut replay = Vec::new();
    loop {
        let frame = again.next(WAIT).expect("replay frames");
        if frame.kind == kind::REPLAY {
            replay.extend_from_slice(&frame.payload);
        } else if frame.kind == kind::REPLAY_END {
            break;
        }
    }

    assert_eq!(replay, live_bytes);
    assert_same_cells(&grid_from(&live_bytes, cols, rows), &grid_from(&replay, cols, rows), cols, rows);
}

#[test]
fn test_c_d4a_idle_exit_removes_daemon_json_and_socket_file() {
    let mut daemon = TestDaemon::start();
    assert!(daemon.info_path().exists());
    // Windows의 명명된 파이프는 프로세스가 끝나면 사라지므로 파일로 확인하는 것은 Unix의 소켓뿐이다.
    #[cfg(unix)]
    let socket_path = PathBuf::from(daemon.wait_for_info().endpoint);
    #[cfg(unix)]
    assert!(socket_path.exists(), "the socket file must exist while the daemon runs");
    // 터미널을 만들지 않으므로 데몬은 유휴 10초가 지나면 끝난다.
    assert!(!daemon.wait_for_exit(Duration::from_secs(8)), "must not exit before the idle period");
    assert!(daemon.wait_for_exit(Duration::from_secs(10)), "must exit once idle");
    assert!(!daemon.info_path().exists(), "daemon.json must be removed on exit");
    #[cfg(unix)]
    assert!(!socket_path.exists(), "the socket file must be removed on exit");
}

#[test]
fn test_c_d4a_new_terminal_cancels_idle_exit() {
    let mut daemon = TestDaemon::start();
    thread::sleep(Duration::from_secs(4));
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 80, 24);
    // 시작부터 센 유휴 시간을 훌쩍 넘겨도 살아 있는 터미널이 있으면 데몬이 남는다.
    thread::sleep(Duration::from_secs(9));
    assert!(daemon.is_running());
    assert!(daemon.info_path().exists());
    assert_eq!(client.list().terminals.len(), 1);
}

#[test]
fn test_c_d4b_second_client_attaches_same_daemon() {
    let daemon = TestDaemon::start();
    let (mut first, first_hello) = daemon.connect_authenticated();
    let attached = first.new_terminal(None, 80, 24);
    type_line(&mut first, ECHO_MARKER);
    first.collect_until(MARKER);

    let (mut second, second_hello) = daemon.connect_authenticated();
    assert_eq!(first_hello.pid, second_hello.pid);
    assert_eq!(second_hello.pid, daemon.pid());
    assert_eq!(second_hello.pid, daemon.read_info().expect("daemon.json").pid);
    second.send(Frame::json(kind::ATTACH, &Attach { id: attached.id, cols: 0, rows: 0 }));
    let replay = second.collect_until(MARKER);
    assert!(String::from_utf8_lossy(&replay).contains(MARKER));
    assert_eq!(second.list().terminals.len(), 1);

    second.send(Frame::new(kind::STATUS_REQUEST, Vec::new()));
    let status: Status = second.expect(kind::STATUS).parse_json().unwrap();
    assert_eq!((status.pid, status.n_terminals), (daemon.pid(), 1));
}

#[test]
fn test_two_daemons_started_together_leave_exactly_one() {
    let mut first = TestDaemon::spawn_without_waiting(&[]);
    let mut rival = first.spawn_rival();
    // 잠금을 얻지 못한 쪽은 오류 종료 코드 2로 끝나고, 잠금을 얻은 쪽만 접속 정보의 주인이 된다.
    let end = Instant::now() + WAIT;
    let mut statuses = (None, None);
    while (statuses.0.is_none() && statuses.1.is_none()) && Instant::now() < end {
        statuses = (first.child.try_wait().unwrap(), rival.try_wait().unwrap());
        thread::sleep(Duration::from_millis(50));
    }
    let (winner_pid, loser_status) = match statuses {
        (Some(status), None) => (rival.id(), status),
        (None, Some(status)) => (first.pid(), status),
        other => panic!("exactly one daemon must exit, got {other:?}"),
    };
    assert_eq!(loser_status.code(), Some(2));
    assert_eq!(first.wait_for_info().pid, winner_pid);
    // 살아남은 데몬은 여전히 인사를 받아 준다.
    let (_client, hello_ok) = first.connect_authenticated();
    assert_eq!(hello_ok.pid, winner_pid);
    let _ = rival.kill();
    let _ = rival.wait();
}

#[test]
fn test_connect_verified_then_hello_on_a_real_stream() {
    let daemon = TestDaemon::start();
    let info = daemon.wait_for_info();
    // 접속한 서버의 소유자를 확인한 뒤에야 인사를 보낸다. 같은 사용자의 실제 데몬은 확인을 통과한다.
    let stream = endpoint::connect_verified(&info.endpoint).expect("same-user server passes the owner check");
    let (mut recv, mut send) = stream.split();
    Frame::json(kind::HELLO, &Hello { token: info.token, protocol_version: PROTOCOL_VERSION })
        .write_to(&mut send)
        .unwrap();
    assert_eq!(Frame::read_from(&mut recv).unwrap().kind, kind::HELLO_OK);
}

#[test]
fn test_c_d18b_breakaway_denied_recorded() {
    let denied = TestDaemon::start_with(&["--breakaway-denied"]);
    assert!(denied.wait_for_info().breakaway_denied);
    drop(denied);

    let normal = TestDaemon::start();
    assert!(!normal.wait_for_info().breakaway_denied);
}

#[test]
fn test_c_t4_daemon_log_records_events_not_input() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 80, 24);
    type_line(&mut client, &format!("echo SECRET''TEXT; {ECHO_MARKER}"));
    client.collect_until(MARKER);

    let mut log = String::new();
    std::fs::File::open(daemon.log_path()).expect("daemon.log").read_to_string(&mut log).unwrap();
    assert!(log.contains("attach"), "{log}");
    assert!(!log.contains("SECRET"), "typed text must never be logged: {log}");
    assert!(!daemon.token_in(&log), "token must never be logged");
}

impl TestDaemon {
    fn token_in(
        &self,
        text: &str,
    ) -> bool {
        text.contains(&self.wait_for_info().token)
    }
}

/// 프로세스가 쓰는 실제 메모리 바이트(Linux는 RSS, Windows는 작업 집합).
fn memory_bytes(pid: u32) -> u64 {
    #[cfg(unix)]
    let bytes = {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("read /proc status");
        let line = status.lines().find(|line| line.starts_with("VmRSS:")).expect("VmRSS line");
        let n_kib: u64 = line.split_whitespace().nth(1).and_then(|text| text.parse().ok()).expect("VmRSS kB");
        n_kib * 1024
    };
    #[cfg(windows)]
    let bytes = unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        assert!(!process.is_null(), "open daemon process");
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(process, &mut counters, counters.cb);
        CloseHandle(process);
        assert!(ok != 0, "read process memory");
        counters.WorkingSetSize as u64
    };
    bytes
}

/// 한 줄이 80바이트인 큰 텍스트 파일을 만든다.
fn write_big_file(
    dir: &Path,
    n_mib: usize,
) -> PathBuf {
    let path = dir.join("big.txt");
    let mut text = String::new();
    let n_lines = n_mib * (1 << 20) / 80;
    for line in 0..n_lines {
        text.push_str(&format!("{line:07} {}\n", "x".repeat(70)));
    }
    std::fs::write(&path, text).expect("write big file");
    path
}

/// 출력 파일 내용을 터미널에 쏟고 끝 표지를 찍는 명령줄.
fn dump_file_command(path: &Path) -> String {
    #[cfg(unix)]
    let command = format!("cat '{}'\n{ECHO_MARKER}", path.display());
    #[cfg(windows)]
    let command = format!("type \"{}\"\r\n{ECHO_MARKER}", path.display());
    command
}

/// 소켓을 읽지 않는 클라이언트 앞에서 큰 출력을 쏟는다. 쏟는 동안 데몬 메모리가 가장 많이 늘어난 바이트 수와 그 클라이언트를 돌려준다.
fn flood_unread_client(
    daemon: &TestDaemon,
    work: &Path,
) -> (Client, u64) {
    let file = write_big_file(work, 32);
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 100, 30);
    // 첫 프롬프트까지 읽은 뒤 읽기를 멈춘다.
    client.collect_until_quiet(Vec::new(), Duration::from_millis(700));
    client.set_paused(true);
    thread::sleep(Duration::from_millis(200));
    let before = memory_bytes(daemon.pid());
    type_line(&mut client, &dump_file_command(&file));
    // 32 MiB가 터미널을 지나가는 동안 메모리를 잰다.
    let end = Instant::now() + Duration::from_secs(8);
    let mut peak = before;
    while Instant::now() < end {
        peak = peak.max(memory_bytes(daemon.pid()));
        thread::sleep(Duration::from_millis(100));
    }
    (client, peak - before)
}

#[test]
fn test_c_d13_unread_client_does_not_grow_daemon_memory() {
    let daemon = TestDaemon::start();
    let work = tempfile::tempdir().expect("work dir");
    let (_client, growth) = flood_unread_client(&daemon, work.path());
    // 32 MiB를 쏟아도 읽지 않는 연결에 쌓는 양은 상한(2 MiB)과 링(1 MiB) 안이어야 한다.
    assert!(growth < 16 << 20, "daemon memory grew by {} MiB", growth >> 20);
}

#[test]
fn test_c_d13_slow_client_gets_a_full_resync_and_matches_the_screen() {
    let (cols, rows) = (100, 30);
    let daemon = TestDaemon::start();
    let work = tempfile::tempdir().expect("work dir");
    let (client, _) = flood_unread_client(&daemon, work.path());
    let terminal_id = daemon.connect_authenticated().0.list().terminals[0].id;

    // 다시 읽기 시작하면 밀린 프레임 뒤에 RESYNC가 오고, 그 뒤 재생과 실시간 출력으로 화면이 이어진다.
    client.set_paused(false);
    let mut stream = Vec::new();
    let mut n_resyncs = 0;
    let mut marker_seen = false;
    // 큰 출력이 끝까지 지나가야 끝 표지가 보인다. Windows의 의사 콘솔은 느려서 넉넉히 기다린다.
    let end = Instant::now() + FLOOD_WAIT;
    while !(n_resyncs > 0 && marker_seen) {
        let left = end.checked_duration_since(Instant::now()).expect("timed out waiting for the resync");
        let frame = client.next(left).expect("connection ended before the resync");
        match frame.kind {
            kind::RESYNC => {
                n_resyncs += 1;
                stream.clear();
                marker_seen = false;
            }
            kind::REPLAY | kind::OUTPUT => {
                stream.extend_from_slice(&frame.payload);
                // 새로 받은 바이트와 그 앞 몇 바이트만 훑는다(전체를 매번 훑으면 시험이 출력보다 느려진다).
                let from = stream.len().saturating_sub(frame.payload.len() + MARKER.len());
                marker_seen = marker_seen || String::from_utf8_lossy(&stream[from..]).contains(MARKER);
            }
            _ => {}
        }
    }
    let stream = client.collect_until_quiet(stream, Duration::from_millis(700));

    // 새 연결이 받는 링 전체가 화면의 정답이다. 다시 맞춘 화면은 그 끝과 같아야 한다.
    let mut fresh = daemon.connect_authenticated().0;
    fresh.send(Frame::json(kind::ATTACH, &Attach { id: terminal_id, cols: 0, rows: 0 }));
    let mut ring = Vec::new();
    loop {
        let frame = fresh.next(WAIT).expect("replay frames");
        if frame.kind == kind::REPLAY {
            ring.extend_from_slice(&frame.payload);
        } else if frame.kind == kind::REPLAY_END {
            break;
        }
    }
    assert!(stream.ends_with(&ring), "resynced stream must end with the ring contents");
    assert_same_cells(&grid_from(&stream, cols, rows), &grid_from(&ring, cols, rows), cols, rows);
}

#[test]
fn test_c_d4a_idle_exit_waits_for_connections_to_close() {
    let mut daemon = TestDaemon::start();
    // 터미널이 없어도 인사를 마친 연결이 있으면 유휴 시간이 지나도 끝나지 않는다.
    let (client, _) = daemon.connect_authenticated();
    thread::sleep(Duration::from_secs(11));
    assert!(daemon.is_running(), "a daemon with a live connection must not exit");
    // 연결이 끊긴 때부터 10초가 지나면 끝난다.
    client.disconnect();
    assert!(!daemon.wait_for_exit(Duration::from_secs(8)), "must not exit right after the last connection closes");
    assert!(daemon.wait_for_exit(Duration::from_secs(6)), "must exit once idle without connections");
}

#[test]
fn test_exit_code_reaches_exited_list_and_late_attach() {
    let daemon = TestDaemon::start();
    let (mut first, _) = daemon.connect_authenticated();
    let attached = first.new_terminal(None, 80, 24);
    #[cfg(unix)]
    type_line(&mut first, "exit 7\n");
    #[cfg(windows)]
    type_line(&mut first, "exit 7\r\n");
    let exited = first.expect(kind::EXITED).as_text();
    assert_eq!(exited, "7");

    // 셸이 끝난 뒤 다른 연결이 붙어도 같은 종료 코드를 받는다.
    let (mut second, _) = daemon.connect_authenticated();
    second.send(Frame::json(kind::ATTACH, &Attach { id: attached.id, cols: 0, rows: 0 }));
    assert_eq!(second.expect(kind::EXITED).as_text(), "7");
    let info = second.list().terminals.into_iter().find(|terminal| terminal.id == attached.id).expect("listed");
    assert!(!info.alive);
    assert_eq!(info.exit_code, Some(7));
}

#[test]
fn test_kill_reports_a_known_exit_code() {
    let daemon = TestDaemon::start();
    let (mut owner, _) = daemon.connect_authenticated();
    let attached = owner.new_terminal(None, 80, 24);
    let (mut other, _) = daemon.connect_authenticated();
    other.send(Frame::json(kind::KILL, &Kill { id: attached.id }));
    other.expect(kind::LIST_REPLY);
    let exited = owner.expect(kind::EXITED).as_text();
    assert_ne!(exited, "unknown", "a killed shell still has an exit status");
    assert!(exited.parse::<i32>().is_ok(), "{exited}");
}

#[test]
fn test_attach_by_another_connection_detaches_the_previous_receiver() {
    let daemon = TestDaemon::start();
    let (mut first, _) = daemon.connect_authenticated();
    let attached = first.new_terminal(None, 80, 24);
    let (mut second, _) = daemon.connect_authenticated();
    second.send(Frame::json(kind::ATTACH, &Attach { id: attached.id, cols: 0, rows: 0 }));
    second.expect(kind::ATTACHED);
    first.expect(kind::DETACHED);

    // 밀려난 연결에는 더 이상 출력이 오지 않고, 새 연결이 출력을 받는다.
    type_line(&mut second, ECHO_MARKER);
    second.collect_until(MARKER);
    let leftover = first.collect_until_quiet(Vec::new(), Duration::from_millis(500));
    assert!(!String::from_utf8_lossy(&leftover).contains(MARKER), "detached connection must not receive output");
}

#[test]
fn test_kill_of_the_attached_terminal_closes_the_connection_and_lets_the_daemon_go_idle() {
    let mut daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = client.new_terminal(None, 80, 24);
    client.send(Frame::json(kind::KILL, &Kill { id: attached.id }));
    let remaining = client.expect(kind::LIST_REPLY).parse_json::<List>().unwrap();
    assert!(remaining.terminals.is_empty());
    // 닫은 탭의 연결이 남으면 데몬이 연결 수 때문에 유휴 종료하지 못한다.
    assert!(client.wait_until_closed(WAIT), "the connection of a killed terminal must be closed by the daemon");
    assert!(daemon.wait_for_exit(Duration::from_secs(14)), "no terminals and no connections: the daemon must go idle");
}

#[test]
fn test_bye_closes_the_connection_without_a_reply() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.send(Frame::new(kind::BYE, Vec::new()));
    assert!(client.wait_until_closed(WAIT));
}

/// 상태 알림 모음에서 같은 값이 연달아 두 번 나오지 않았는지(완료 알림 프레임은 예외).
fn assert_no_consecutive_duplicates(states: &[TerminalState]) {
    for pair in states.windows(2) {
        let repeated = TerminalState { finished: false, ..pair[0].clone() }
            == TerminalState { finished: false, ..pair[1].clone() };
        assert!(!repeated || pair[1].finished, "the same state was sent twice in a row: {pair:?}");
    }
}

#[test]
fn test_c_d37_state_arrives_on_attach_and_is_sent_only_on_change() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = client.new_terminal(None, 80, 24);
    let mut seen = Vec::new();
    // 붙는 순간 지금 상태가 오고, 첫 틱의 조사가 셸의 이름을 밝힌다.
    let known = client.wait_for_state(Duration::from_secs(10), &mut seen, |state| state.program.is_some());
    let known = known.expect("the first probe names the shell");
    assert_eq!(known.id, attached.id);
    assert!(!known.is_agent);
    // 같은 출력이 반복돼도(셸이라 상태가 그대로) 프레임이 더 오지 않는다.
    for _ in 0..4 {
        type_line(&mut client, ECHO_MARKER);
        thread::sleep(Duration::from_millis(400));
    }
    let after = client.collect_states(Duration::from_secs(5));
    assert!(after.is_empty(), "no state change, no frame: {after:?}");
    assert_no_consecutive_duplicates(&seen);
}

#[test]
fn test_c_d37_list_carries_the_state() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = client.new_terminal(None, 80, 24);
    let mut seen = Vec::new();
    client.wait_for_state(Duration::from_secs(10), &mut seen, |state| state.program.is_some()).expect("shell known");
    let (mut other, _) = daemon.connect_authenticated();
    let listed = other.list().terminals.into_iter().find(|terminal| terminal.id == attached.id).expect("listed");
    assert!(listed.state.is_some_and(|state| state.program.is_some()));
}

#[cfg(unix)]
#[test]
fn test_c_d37_linux_foreground_is_read_from_proc() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 80, 24);
    type_line(&mut client, "sleep 30\n");
    let mut seen = Vec::new();
    let state =
        client.wait_for_state(Duration::from_secs(10), &mut seen, |state| state.program.as_deref() == Some("sleep"));
    assert!(state.is_some(), "a running sleep becomes the foreground program: {seen:?}");
    assert!(state.is_some_and(|state| !state.is_agent && state.activity.is_none()));
    assert!(seen.iter().any(|state| state.cwd.is_some()), "the shell's folder is read from /proc: {seen:?}");
}

#[cfg(unix)]
#[test]
fn test_c_d37_an_agent_named_program_gets_a_state_and_a_finish_is_announced_once() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 100, 24);
    // 에이전트 이름의 실행 파일(claude)을 PATH 앞에 두고, 6초 동안 출력을 낸 뒤 끝나는 일을 시킨다.
    // 마지막 조용한 4초는 셸 내장 명령(read)으로 기다린다. `bash -c`는 마지막 명령이 외부 프로그램(sleep)이면 새로 띄우지 않고 그 프로그램으로 자신을 바꿔 실행(exec)하므로,
    // 가짜 claude 프로세스 자체가 sleep이 되어 기록을 쓰기 직전의 조사가 앞쪽 프로그램을 sleep으로 읽는다.
    let bin = daemon.home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink("/bin/bash", bin.join("claude")).unwrap();
    type_line(&mut client, &format!("PATH={}:$PATH\r", bin.display()));
    type_line(
        &mut client,
        "claude -c 'for i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo work $i; sleep 0.5; done; read -t 4 -r _'\r",
    );
    let mut seen = Vec::new();
    let finished = client.wait_for_state(Duration::from_secs(30), &mut seen, |state| state.finished);
    let finished = finished.unwrap_or_else(|| panic!("the finish was never announced: {seen:?}"));
    assert_eq!(finished.program.as_deref(), Some("claude"));
    assert!(finished.is_agent);
    assert!(
        seen.iter().any(|state| state.activity == Some(paddock_core::agent_model::ActivityState::Working)),
        "{seen:?}"
    );
    assert_eq!(seen.iter().filter(|state| state.finished).count(), 1);
    assert_no_consecutive_duplicates(&seen);
}

#[cfg(unix)]
#[test]
fn test_c_d37_osc7501_query_is_answered_on_the_pty() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 120, 24);
    // 셸이 질의를 출력하면 데몬이 같은 시퀀스를 터미널 입력으로 돌려준다. 읽은 10바이트를 16진수로 찍어 확인한다.
    type_line(
        &mut client,
        "printf '\\033]7501;?\\033\\\\'; read -r -s -N 10 -t 8 reply; printf 'GOT:%s\\n' \"$(printf %s \"$reply\" | od -An -tx1 | tr -d ' \\n')\"\n",
    );
    let output = String::from_utf8_lossy(&client.collect_until("GOT:1b5d373530313b3f1b5c")).into_owned();
    assert!(output.contains("GOT:1b5d373530313b3f1b5c"), "{output}");
}

#[cfg(unix)]
#[test]
fn test_c_d37_osc7_with_a_korean_path_does_not_stop_the_terminal() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 120, 24);
    // 폴더 알림의 앞 7바이트 안에서 한글이 잘려도 출력 처리 스레드가 죽지 않아 뒤따르는 출력이 계속 온다.
    type_line(&mut client, "printf '\\033]7;/tmp/작\\007'; printf '\\033]7;/home/작업\\007'; echo AFTER''KOREAN\n");
    let output = String::from_utf8_lossy(&client.collect_until("AFTERKOREAN")).into_owned();
    assert!(output.contains("AFTERKOREAN"), "{output}");
    type_line(&mut client, "echo STILL''ALIVE\n");
    let output = String::from_utf8_lossy(&client.collect_until("STILLALIVE")).into_owned();
    assert!(output.contains("STILLALIVE"), "{output}");
}

#[test]
fn test_new_with_launch_runs_that_program_instead_of_a_shell() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    #[cfg(unix)]
    let launch = Launch {
        program: "bash".into(),
        args: vec!["-c".into(), "printf 'LAUN%s\\n' CHED; sleep 5".into()],
        env: vec![("PADDOCK_TEST_VALUE".into(), "kept".into())],
        cwd: None,
        title: None,
        ..Launch::default()
    };
    #[cfg(windows)]
    let launch = Launch {
        program: "cmd".into(),
        args: vec!["/c".into(), "echo LAUN^CHED & ping -n 6 127.0.0.1 > nul".into()],
        env: Vec::new(),
        cwd: None,
        title: None,
        ..Launch::default()
    };
    let request = New {
        shell: SHELL.into(),
        cwd: None,
        cols: 80,
        rows: 24,
        wsl_distribution: None,
        launch: Some(launch),
        preload: None,
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    client.expect(kind::ATTACHED);
    let output = client.collect_until("LAUNCHED");
    assert!(String::from_utf8_lossy(&output).contains("LAUNCHED"));
}

/// 앱이 다시 붙을 때 넘기는 최신 PATH가 새 일반 터미널에 간다. 데몬 자신의 PATH에는 없는 폴더로 확인한다.
#[test]
fn test_c_d7_new_terminal_gets_the_apps_latest_path() {
    const NEW_FOLDER: &str = "paddock-app-latest-path-folder";
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let (name, separator) = if cfg!(windows) { ("Path", ';') } else { ("PATH", ':') };
    let inherited = std::env::var(name).unwrap_or_default();
    let request = New {
        shell: SHELL.into(),
        cwd: None,
        cols: 200,
        rows: 24,
        wsl_distribution: None,
        launch: None,
        preload: None,
        env: vec![(name.to_string(), format!("{NEW_FOLDER}{separator}{inherited}"))],
    };
    client.send(Frame::json(kind::NEW, &request));
    client.expect(kind::ATTACHED);
    #[cfg(unix)]
    type_line(&mut client, "echo \"FOUND:$PATH\"\n");
    #[cfg(windows)]
    type_line(&mut client, "echo FOUND:%PATH%\r\n");
    let output = String::from_utf8_lossy(&client.collect_until(&format!("FOUND:{NEW_FOLDER}"))).into_owned();
    assert!(output.contains(&format!("FOUND:{NEW_FOLDER}")), "{output}");
}

#[cfg(windows)]
#[test]
fn test_c_d37_windows_foreground_is_read_from_the_process_list() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 100, 24);
    type_line(&mut client, "ping -t 127.0.0.1\r");
    let mut seen = Vec::new();
    let state =
        client.wait_for_state(Duration::from_secs(15), &mut seen, |state| state.program.as_deref() == Some("ping"));
    assert!(state.is_some(), "ping.exe becomes the foreground program of the cmd shell: {seen:?}");
}

#[cfg(windows)]
#[test]
fn test_c_d37_powershell_reports_its_folder_with_osc7_and_names_its_foreground_program() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let request = New {
        shell: "powershell".into(),
        cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
        cols: 100,
        rows: 24,
        wsl_distribution: None,
        launch: None,
        preload: None,
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    client.expect(kind::ATTACHED);
    let mut seen = Vec::new();
    let with_folder = client.wait_for_state(Duration::from_secs(30), &mut seen, |state| state.cwd.is_some());
    assert!(with_folder.is_some(), "the PowerShell prompt reports the current folder: {seen:?}");
    type_line(&mut client, "ping -t 127.0.0.1\r");
    let is_ping =
        |state: &TerminalState| state.program.as_deref().is_some_and(|name| name.eq_ignore_ascii_case("ping"));
    let state = client.wait_for_state(Duration::from_secs(20), &mut seen, is_ping);
    assert!(state.is_some(), "{seen:?}");
}

#[cfg(unix)]
/// 시험용 가짜 계정. 로그인 값은 없고 식별자·도구·이름·실행 환경만 있다.
fn fake_account() -> AccountProfile {
    AccountProfile {
        id: "00000000-0000-4000-8000-000000000001".into(),
        provider: "claude".into(),
        label: "Work Claude".into(),
        runtime: Runtime::Native,
        wsl_distribution: None,
    }
}

#[cfg(unix)]
#[test]
fn test_c_d39_launch_env_has_no_login_values() {
    const SECRET: &str = "sk-test-secret-value-must-not-leak";
    // 로그인을 우회할 수 있는 변수를 데몬 환경에 일부러 둔다. 계정 터미널의 자식 환경에는 하나도 남지 않아야 한다.
    let mut secrets: Vec<(&str, &str)> = AUTH_ENV.iter().map(|name| (*name, SECRET)).collect();
    // 이 컴퓨터에 설치된 실제 claude가 시작 파일에 잡히지 않도록 기본 도구 폴더만 PATH에 둔다.
    secrets.push(("PATH", "/usr/bin:/bin"));
    let daemon = TestDaemon::start_with_env(&secrets);
    let account = fake_account();
    let config_dir = daemon.home.path().join("account folder's");
    std::fs::create_dir_all(&config_dir).unwrap();
    let cwd = daemon.home.path().to_string_lossy().into_owned();
    let launch = account_terminal_spec(&AccountLaunchInput {
        profile: &account,
        config_dir: Location::Absolute(config_dir.to_string_lossy().into_owned()),
        cwd: &cwd,
        is_windows: false,
        permissions: Permissions::Ask,
        flag_settings_file: None,
        resume_session_id: None,
    })
    .expect("launch spec");
    // 시작 파일의 `unset`과 별개로, 데몬이 띄운 첫 프로세스의 환경에도 로그인 변수가 없어야 한다.
    let dump = daemon.home.path().join("first-process-env.txt");
    let direct = Launch {
        program: "bash".into(),
        args: vec!["-c".into(), "env > \"$1\"; sleep 5".into(), "_".into(), dump.to_string_lossy().into_owned()],
        env: vec![("CLAUDE_CONFIG_DIR".into(), config_dir.to_string_lossy().into_owned())],
        env_remove: AUTH_ENV.iter().map(ToString::to_string).collect(),
        ..Launch::default()
    };
    let (mut first, _) = daemon.connect_authenticated();
    let request = New {
        shell: SHELL.into(),
        cwd: None,
        cols: 80,
        rows: 24,
        wsl_distribution: None,
        launch: Some(direct),
        preload: None,
        env: Vec::new(),
    };
    first.send(Frame::json(kind::NEW, &request));
    first.expect(kind::ATTACHED);
    let end = Instant::now() + WAIT;
    while !dump.exists() && Instant::now() < end {
        thread::sleep(Duration::from_millis(50));
    }
    thread::sleep(Duration::from_millis(200));
    let environment = std::fs::read_to_string(&dump).expect("the first process wrote its environment");
    assert!(AUTH_ENV.iter().all(|name| !environment.contains(&format!("{name}="))), "{environment}");
    assert!(environment.contains(&format!("CLAUDE_CONFIG_DIR={}", config_dir.display())));
    assert!(!environment.contains(SECRET));
    let (mut client, _) = daemon.connect_authenticated();
    let request = New {
        shell: SHELL.into(),
        cwd: Some(cwd.clone()),
        cols: 200,
        rows: 24,
        wsl_distribution: None,
        launch: Some(launch),
        preload: None,
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    client.expect(kind::ATTACHED);
    // 시작 파일이 끝난 셸에서 로그인 변수 수와 계정 폴더 변수를 읽는다. 명령줄 에코에는 `COUNT=0`이 나타나지 않는다.
    let pattern = AUTH_ENV.join("|");
    type_line(
        &mut client,
        &format!(
            "printf 'COUNT=%s\\n' \"$(env | grep -c -E '^({pattern})=')\"; printf 'DIR=[%s]\\n' \"$CLAUDE_CONFIG_DIR\"\n"
        ),
    );
    let output = String::from_utf8_lossy(&client.collect_until("DIR=[/")).into_owned();
    let output = String::from_utf8_lossy(&client.collect_until_quiet(output.into_bytes(), Duration::from_millis(500)))
        .into_owned();
    assert!(output.contains("COUNT=0"), "{output}");
    assert!(output.contains(&format!("DIR=[{}]", config_dir.display())), "{output}");
    assert!(!output.contains(SECRET), "login values must not reach the terminal: {output}");
    // 목록에는 계정 이름만 실리고, 데몬 기록에는 로그인 값이 없다.
    let listed = client.list().terminals;
    assert_eq!(listed.iter().filter_map(|terminal| terminal.account.as_ref()).collect::<Vec<_>>(), [&account]);
    let log = std::fs::read_to_string(daemon.log_path()).unwrap_or_default();
    assert!(!log.contains(SECRET), "{log}");
    let mut seen = Vec::new();
    let state = client.wait_for_state(Duration::from_secs(10), &mut seen, |state| {
        state
            .account
            .as_ref()
            .and_then(|scope| scope.profile.as_ref())
            .is_some_and(|profile| profile.label == "Work Claude")
    });
    assert!(state.is_some(), "the screen scope names the account that opened the terminal: {seen:?}");
}

/// `printf 'P=[%s]END''MARK\n' "$PATH"`를 쳐서 터미널의 PATH를 읽는다.
#[cfg(unix)]
fn terminal_path(client: &mut Client) -> String {
    type_line(client, "printf 'P=[%s]END''MARK\\n' \"$PATH\"\n");
    let output = String::from_utf8_lossy(&client.collect_until("]ENDMARK")).into_owned();
    let start = output.rfind("P=[").expect("path marker");
    output[start + 3..output.rfind("]ENDMARK").unwrap()].to_string()
}

#[cfg(unix)]
#[test]
fn test_c_d39_path_gets_wrapper_dir_only_when_it_exists() {
    let daemon = TestDaemon::start();
    let wrapper = daemon.home.path().join(".paddock").join("config").join("bin");
    let (mut client, _) = daemon.connect_authenticated();
    client.new_terminal(None, 400, 24);
    let without = terminal_path(&mut client);
    assert!(!without.contains(wrapper.to_str().unwrap()), "no wrapper folder, no PATH entry: {without}");
    std::fs::create_dir_all(&wrapper).unwrap();
    let (mut second, _) = daemon.connect_authenticated();
    second.new_terminal(None, 400, 24);
    let with = terminal_path(&mut second);
    assert!(with.starts_with(&format!("{}:", wrapper.display())), "the wrapper folder comes first: {with}");
    // 이미 떠 있던 터미널의 PATH는 바뀌지 않는다.
    assert_eq!(terminal_path(&mut client), without);
}

// ---- 터미널 기록(terminal-history) ----

/// 데몬이 쓰는 기록 폴더 루트.
fn history_root(home: &Path) -> PathBuf {
    home.join(".paddock").join("native").join("terminal-history")
}

/// 기록 루트 아래 모든 데몬 폴더의 `<번호>.json` 파일들.
fn history_files(home: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for folder in std::fs::read_dir(history_root(home)).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(folder.path()).into_iter().flatten().flatten() {
            if file.path().extension().is_some_and(|extension| extension == "json") {
                files.push(file.path());
            }
        }
    }
    files
}

/// 터미널 `id`의 기록을 `accept`가 만족할 때까지 기다려 돌려준다.
fn wait_for_record(
    home: &Path,
    id: u32,
    accept: impl Fn(&HistoryRecord) -> bool,
) -> HistoryRecord {
    let end = Instant::now() + Duration::from_secs(25);
    let name = format!("{id}.json");
    let mut found = None;
    while found.is_none() {
        let record = history_files(home)
            .into_iter()
            .find(|file| file.file_name().is_some_and(|candidate| candidate == name.as_str()))
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| parse_record(&text))
            .filter(|record| accept(record));
        match record {
            Some(record) => found = Some(record),
            None => {
                assert!(
                    Instant::now() < end,
                    "no matching history record for terminal {id}: {:?}",
                    history_files(home)
                );
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    found.unwrap()
}

/// 터미널을 열고 표지가 찍힐 때까지 읽은 뒤 터미널 정보를 돌려준다.
fn terminal_with_marker(
    client: &mut Client,
    cwd: Option<&str>,
) -> Attached {
    let attached = client.new_terminal(cwd, 100, 30);
    type_line(client, ECHO_MARKER);
    client.collect_until(MARKER);
    attached
}

#[test]
fn test_c_d11c_tail_is_bounded_to_256kb() {
    let daemon = TestDaemon::start();
    let work = tempfile::tempdir().expect("work dir");
    let file = write_big_file(work.path(), 1);
    let (mut client, _) = daemon.connect_authenticated();
    let attached = client.new_terminal(None, 100, 30);
    type_line(&mut client, &dump_file_command(&file));
    client.collect_until(MARKER);
    let record = wait_for_record(daemon.home.path(), attached.id, |record| record.output.contains(MARKER));
    assert!(record.output.len() <= 256 * 1024, "tail is {} bytes", record.output.len());
    assert!(
        record.output.len() > 128 * 1024,
        "1 MiB of output leaves a substantial tail: {} bytes",
        record.output.len()
    );
    let total: u64 = history_files(daemon.home.path()).iter().map(|file| std::fs::metadata(file).unwrap().len()).sum();
    assert!(total <= 512 * 1024, "one terminal stays within 512 KiB in total: {total}");
    // 기록 파일은 진단용으로 공유될 수 있는 daemon.log와 섞이지 않는다 — 화면 출력은 로그에 없다.
    let log = std::fs::read_to_string(daemon.log_path()).unwrap_or_default();
    assert!(!log.contains(MARKER) && !log.contains("xxxxxxxxxx"), "terminal output must not reach the daemon log");
}

#[test]
fn test_c_d40_kill_deletes_history_but_shutdown_keeps_it() {
    let mut daemon = TestDaemon::start();
    let home = daemon.home.path().to_path_buf();
    let (mut client, _) = daemon.connect_authenticated();
    let closed = terminal_with_marker(&mut client, None);
    let (mut other, _) = daemon.connect_authenticated();
    let kept = terminal_with_marker(&mut other, None);
    wait_for_record(&home, closed.id, |record| record.output.contains(MARKER));
    wait_for_record(&home, kept.id, |record| record.output.contains(MARKER));
    // 사용자가 끝낸 터미널의 기록은 지워진다.
    client.send(Frame::json(kind::KILL, &Kill { id: closed.id }));
    client.expect(kind::LIST_REPLY);
    let end = Instant::now() + Duration::from_secs(10);
    while history_files(&home)
        .iter()
        .any(|file| file.file_name().is_some_and(|name| name == format!("{}.json", closed.id).as_str()))
    {
        assert!(Instant::now() < end, "the closed terminal's record is still there");
        thread::sleep(Duration::from_millis(100));
    }
    // 종료 요청으로 셸이 모두 끝나도 남은 터미널의 기록은 남는다.
    other.send(Frame::new(kind::SHUTDOWN, Vec::new()));
    other.expect(kind::STATUS);
    assert!(daemon.wait_for_exit(Duration::from_secs(10)), "the daemon exits after SHUTDOWN");
    thread::sleep(Duration::from_millis(500));
    let names: Vec<String> = history_files(&home)
        .iter()
        .filter_map(|file| file.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![format!("{}.json", kept.id)]);
}

#[test]
fn test_c_d40_history_is_flushed_before_shutdown() {
    let mut daemon = TestDaemon::start();
    let home = daemon.home.path().to_path_buf();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = terminal_with_marker(&mut client, None);
    // 출력이 멎고 2초가 지나기 전에 종료한다: 내려쓰지 않으면 기록이 없다.
    client.send(Frame::new(kind::SHUTDOWN, Vec::new()));
    client.expect(kind::STATUS);
    assert!(daemon.wait_for_exit(Duration::from_secs(10)));
    let text = std::fs::read_to_string(
        history_root(&home)
            .join(
                std::fs::read_dir(history_root(&home))
                    .expect("history folder")
                    .flatten()
                    .next()
                    .expect("daemon folder")
                    .file_name(),
            )
            .join(format!("{}.json", attached.id)),
    )
    .expect("the record written at shutdown");
    let record = parse_record(&text).expect("a readable record");
    assert!(record.output.contains(MARKER), "the last output reached the file: {:?}", record.output);
}

#[test]
fn test_c_d40_never_setting_writes_nothing_and_removes_existing() {
    let daemon = TestDaemon::start_after(|home| {
        let native = home.join(".paddock").join("native");
        std::fs::create_dir_all(history_root(home).join("old-daemon")).unwrap();
        std::fs::write(history_root(home).join("old-daemon").join("1.json"), "{}").unwrap();
        std::fs::write(native.join("settings.json"), r#"{ "paddock.terminal.restoreOnRestart": "never" }"#).unwrap();
    });
    let (mut client, _) = daemon.connect_authenticated();
    terminal_with_marker(&mut client, None);
    // 저장 시점(2초)이 지난 뒤에도 기록이 하나도 없고 있던 것도 지워졌다.
    thread::sleep(Duration::from_secs(5));
    assert!(history_files(daemon.home.path()).is_empty(), "{:?}", history_files(daemon.home.path()));
    assert!(!history_root(daemon.home.path()).join("old-daemon").exists());
}

#[cfg(unix)]
#[test]
fn test_c_t4a_history_dir_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = terminal_with_marker(&mut client, None);
    wait_for_record(daemon.home.path(), attached.id, |record| record.output.contains(MARKER));
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let file = history_files(daemon.home.path()).remove(0);
    assert_eq!(mode(&file), 0o600, "a record can hold secrets printed on screen");
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    assert_eq!(mode(&history_root(daemon.home.path())), 0o700);
}

#[test]
fn test_c_d40_history_of_daemons_older_than_14_days_is_deleted_at_start() {
    let daemon = TestDaemon::start_after(|home| {
        let root = history_root(home);
        for name in ["stale-daemon", "recent-daemon"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
            std::fs::write(root.join(name).join("1.json"), "{}").unwrap();
        }
        let old = std::time::SystemTime::now() - Duration::from_secs(15 * 24 * 60 * 60);
        // 폴더 자신의 시각은 파일이 있으면 보지 않으므로 파일 시각만 바꾼다(Windows는 폴더를 파일처럼 열 수 없다).
        let file = std::fs::OpenOptions::new().write(true).open(root.join("stale-daemon").join("1.json")).unwrap();
        file.set_modified(old).unwrap();
    });
    let root = history_root(daemon.home.path());
    let end = Instant::now() + Duration::from_secs(10);
    while root.join("stale-daemon").exists() {
        assert!(Instant::now() < end, "the 15 day old folder was not deleted");
        thread::sleep(Duration::from_millis(100));
    }
    assert!(root.join("recent-daemon").exists(), "a recent folder is kept");
}

#[test]
fn test_c_d40_preload_is_the_first_screen_and_is_replayed_to_a_later_connection() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let request = New {
        shell: SHELL.into(),
        cwd: None,
        cols: 100,
        rows: 30,
        wsl_distribution: None,
        launch: None,
        preload: Some("RESTORED-SCREEN\r\n".into()),
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    let attached: Attached = client.expect(kind::ATTACHED).parse_json().unwrap();
    let mut first = Vec::new();
    loop {
        let frame = client.next(WAIT).expect("replay");
        if frame.kind == kind::REPLAY {
            first.extend_from_slice(&frame.payload);
        } else if frame.kind == kind::REPLAY_END {
            break;
        }
    }
    assert!(first.starts_with(b"RESTORED-SCREEN\r\n"), "{:?}", String::from_utf8_lossy(&first));
    let (mut again, _) = daemon.connect_authenticated();
    again.send(Frame::json(kind::ATTACH, &Attach { id: attached.id, cols: 0, rows: 0 }));
    again.expect(kind::ATTACHED);
    let replay = again.collect_until("RESTORED-SCREEN");
    assert!(replay.starts_with(b"RESTORED-SCREEN"), "the restored screen survives a reattach");
}

#[cfg(unix)]
#[test]
fn test_c_d40_claude_conversation_and_folder_are_recorded() {
    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
    let daemon = TestDaemon::start();
    let home = daemon.home.path().to_path_buf();
    let (mut client, _) = daemon.connect_authenticated();
    let work = home.join("work dir");
    std::fs::create_dir_all(&work).unwrap();
    let attached = client.new_terminal(Some(work.to_str().unwrap()), 120, 24);
    // 에이전트 이름의 실행 파일(claude)이 자기 pid로 Claude의 대화 기록을 남기고 30초 머문다.
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let sessions = home.join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let script = bin.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nprintf '{{\"pid\":%s,\"sessionId\":\"{SESSION}\",\"kind\":\"interactive\"}}' $$ > '{}'/$$.json\necho CLAUDEUP\nsleep 30\n",
            sessions.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    type_line(&mut client, &format!("PATH={}:$PATH\r", bin.display()));
    type_line(&mut client, "claude\r");
    client.collect_until("CLAUDEUP");
    let record = wait_for_record(&home, attached.id, |record| record.agent.is_some());
    let agent = record.agent.expect("the conversation");
    assert_eq!((agent.provider.as_str(), agent.session_id.as_str()), ("claude", SESSION));
    assert_eq!(record.foreground.as_deref(), Some("claude"));
    assert_eq!(record.cwd_start.as_deref(), Some(work.to_str().unwrap()));
    assert_eq!(record.shell, "bash");
}

#[cfg(unix)]
#[test]
fn test_c_d46_shutdown_reads_the_latest_conversation_before_writing_the_record() {
    const SESSION: &str = "0b9f2f77-6a1e-4a4c-9d33-2f7a5d1c1a10";
    let mut daemon = TestDaemon::start();
    let home = daemon.home.path().to_path_buf();
    let (mut client, _) = daemon.connect_authenticated();
    let attached = client.new_terminal(Some(home.to_str().unwrap()), 120, 24);
    // 에이전트가 시작 3초 뒤에 대화 기록을 남기고, 바로 이어 한 줄을 출력한 뒤 조용해진다.
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let sessions = home.join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let script = bin.join("claude");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\necho CLAUDEUP\nsleep 3\nprintf '{{\"pid\":%s,\"sessionId\":\"{SESSION}\",\"kind\":\"interactive\"}}' $$ > '{}'/$$.json\necho FILEMADE\nsleep 60\n",
            sessions.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    type_line(&mut client, &format!("PATH={}:$PATH\r", bin.display()));
    type_line(&mut client, "claude\r");
    client.collect_until("FILEMADE");
    // 대화 기록이 생긴 직후, 틱이 다시 조사하기 전에 종료한다: 쓰기 직전에 최신 상태를 읽지 않으면 대화가 빠진다.
    client.send(Frame::new(kind::SHUTDOWN, Vec::new()));
    client.expect(kind::STATUS);
    assert!(daemon.wait_for_exit(Duration::from_secs(10)));
    let record = wait_for_record(&home, attached.id, |_| true);
    let agent = record.agent.expect("the conversation made just before shutdown");
    assert_eq!((agent.provider.as_str(), agent.session_id.as_str()), ("claude", SESSION));
}

#[test]
fn test_c_d46_the_restored_screen_is_shown_but_never_becomes_part_of_the_next_record() {
    let mut daemon = TestDaemon::start();
    let home = daemon.home.path().to_path_buf();
    let (mut client, _) = daemon.connect_authenticated();
    let request = New {
        shell: SHELL.into(),
        cwd: None,
        cols: 100,
        rows: 30,
        wsl_distribution: None,
        launch: None,
        preload: Some("RESTORED-SCREEN\r\n".into()),
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    let attached: Attached = client.expect(kind::ATTACHED).parse_json().unwrap();
    type_line(&mut client, ECHO_MARKER);
    client.collect_until(MARKER);
    client.send(Frame::new(kind::SHUTDOWN, Vec::new()));
    client.expect(kind::STATUS);
    assert!(daemon.wait_for_exit(Duration::from_secs(10)));
    let record = wait_for_record(&home, attached.id, |_| true);
    assert!(record.output.contains(MARKER), "{:?}", record.output);
    assert!(!record.output.contains("RESTORED-SCREEN"), "the old screen would pile up on every restart");
}

#[test]
fn test_c_d46_a_failed_start_logs_the_error_kind_but_not_the_folder_path() {
    let daemon = TestDaemon::start();
    let (mut client, _) = daemon.connect_authenticated();
    let missing = daemon.home.path().join("private-project-folder-that-is-gone");
    let request = New {
        shell: SHELL.into(),
        cwd: Some(missing.to_string_lossy().into_owned()),
        cols: 80,
        rows: 24,
        wsl_distribution: None,
        launch: None,
        preload: None,
        env: Vec::new(),
    };
    client.send(Frame::json(kind::NEW, &request));
    let reply = client.expect(kind::ERR);
    // 사용자 화면에 보일 안내에는 경로가 있어도 된다.
    assert!(String::from_utf8_lossy(&reply.payload).contains("private-project-folder-that-is-gone"));
    thread::sleep(Duration::from_millis(300));
    let log = std::fs::read_to_string(daemon.log_path()).unwrap_or_default();
    assert!(log.contains("terminal start failed"), "{log}");
    assert!(!log.contains("private-project-folder-that-is-gone"), "the log must not carry folder paths: {log}");
}
