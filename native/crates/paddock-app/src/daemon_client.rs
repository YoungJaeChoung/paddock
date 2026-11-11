//! 앱 쪽 데몬 연결: 데몬을 찾거나 띄우고, 상대가 같은 사용자의 데몬임을 확인한 뒤 인사하고, 프레임을 주고받는다.
//!
//! 연결 하나는 터미널 하나에 묶인다. 앱은 터미널마다 연결을 하나씩 연다.
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Stream as _;
use paddock_protocol::daemon_info::{
    DaemonInfo, ensure_private_dir, native_dir, read_info, remove_info_of, remove_info_owned_by,
};
use paddock_protocol::endpoint;
use paddock_protocol::fingerprint::fingerprint_of;
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Hello, HelloError, HelloOk, PROTOCOL_VERSION, Status, kind};
use paddock_protocol::peer::PeerError;

use crate::app_log;
use crate::daemon_launch::{launch_daemon, terminate_daemon};

/// 기다리는 시간들.
mod timeout {
    use std::time::Duration;

    /// 연결·소유자 확인·인사 응답을 합쳐 기다리는 최대 시간. 가짜 서버가 말없이 붙잡거나 데몬이 멈춰 앱이 멈추는 일을 막는다.
    pub const HANDSHAKE: Duration = Duration::from_secs(5);
    /// 새로 띄운 데몬이 `daemon.json`을 쓰고 연결을 받기를 기다리는 최대 시간.
    pub const DAEMON_START: Duration = Duration::from_secs(8);
    /// 교체할 옛 데몬이 종료 요청에 답하기를 기다리는 시간.
    pub const SHUTDOWN_REPLY: Duration = Duration::from_secs(2);
    /// 데몬이 상태 요청에 답하기를 기다리는 시간.
    pub const STATUS_REPLY: Duration = Duration::from_secs(2);
    /// 종료를 요청한 뒤 데몬 프로세스가 사라지기를 기다리는 최대 시간.
    pub const SHUTDOWN_EXIT: Duration = Duration::from_secs(5);
    /// 기다리며 다시 확인하는 간격.
    pub const POLL: Duration = Duration::from_millis(30);
}

/// 받았지만 아직 격자에 먹이지 못한 출력·재생 바이트의 상한. 넘으면 소켓 읽기를 멈춰 데몬 쪽 상한(밀린 프레임이
/// 차면 출력을 버리고 `RESYNC`로 화면 전체를 다시 보냄)이 이어받게 한다.
const MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;

/// 보내는 통로가 쥐는 항목.
enum Outgoing {
    Frame(Frame),
    /// 앞서 보낸 프레임이 모두 쓰인 뒤 알려 달라는 표시.
    Flush(Sender<()>),
}

/// 연결에 프레임을 보내는 통로. 복제해서 여러 곳(입력·크기 변경)에서 쓴다.
///
/// 프레임은 쓰기 전용 스레드가 순서대로 소켓에 쓴다. 상대가 읽지 않아 쓰기가 막혀도 보내는 쪽(화면 스레드)은 멈추지 않는다.
#[derive(Clone)]
pub struct Outbox(Sender<Outgoing>);

impl Outbox {
    /// `writer`에 쓰는 스레드를 시작한다. 쓰기가 실패하면 스레드가 끝나고 이후 `send`는 오류다.
    pub fn spawn<W: Write + Send + 'static>(mut writer: W) -> Self {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            while let Ok(outgoing) = receiver.recv() {
                match outgoing {
                    Outgoing::Frame(frame) => {
                        if frame.write_to(&mut writer).is_err() {
                            break;
                        }
                    }
                    Outgoing::Flush(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        });
        Self(sender)
    }

    /// 프레임을 보낼 줄에 세운다. 쓰기가 끝나기를 기다리지 않는다.
    pub fn send(
        &self,
        frame: &Frame,
    ) -> io::Result<()> {
        self.0
            .send(Outgoing::Frame(frame.clone()))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Connection to the daemon is closed"))
    }

    /// 지금까지 세운 프레임이 모두 쓰일 때까지 기다린다. 시간 안에 끝나면 true다. 프로세스를 끝내기 직전에 쓴다.
    pub fn flush(
        &self,
        wait: Duration,
    ) -> bool {
        let (ack, acked) = channel();
        self.0.send(Outgoing::Flush(ack)).is_ok() && acked.recv_timeout(wait).is_ok()
    }
}

/// 바이트 상한 계산 대상인 프레임인가(화면을 채우는 큰 프레임).
fn is_bulk(kind: u8) -> bool {
    kind == kind::OUTPUT || kind == kind::REPLAY
}

/// 받는 큐가 쥔 바이트 수와 닫힘 표시.
struct Budget {
    state: Mutex<BudgetState>,
    freed: Condvar,
    limit: usize,
}

struct BudgetState {
    queued: usize,
    closed: bool,
}

/// 소켓에서 읽은 프레임을 큐에 넣는 쪽.
pub struct InboxSender {
    sender: Sender<Frame>,
    budget: Arc<Budget>,
}

/// 받은 프레임을 꺼내는 쪽. 큰 프레임(출력·재생)의 바이트 합이 상한을 넘으면 넣는 쪽이 멈춘다.
pub struct Inbox {
    receiver: Receiver<Frame>,
    budget: Arc<Budget>,
}

/// 바이트 상한이 `limit_bytes`인 받는 큐를 만든다.
pub fn inbox_channel(limit_bytes: usize) -> (InboxSender, Inbox) {
    let (sender, receiver) = channel();
    let budget = Arc::new(Budget {
        state: Mutex::new(BudgetState { queued: 0, closed: false }),
        freed: Condvar::new(),
        limit: limit_bytes,
    });
    (InboxSender { sender, budget: budget.clone() }, Inbox { receiver, budget })
}

impl InboxSender {
    /// 프레임을 큐에 넣는다. 큰 프레임은 큐의 바이트 합이 상한을 넘게 되면 소비가 일어날 때까지 기다린다(큐가 비어 있으면
    /// 상한보다 큰 프레임도 들어간다). 제어 프레임은 기다리지 않는다. 받는 쪽이 사라졌으면 false다.
    pub fn push(
        &self,
        frame: Frame,
    ) -> bool {
        let n_bytes = if is_bulk(frame.kind) { frame.payload.len() } else { 0 };
        let mut state = self.budget.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while !state.closed && n_bytes > 0 && state.queued > 0 && state.queued + n_bytes > self.budget.limit {
            state = self.budget.freed.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        let accepted = !state.closed && self.sender.send(frame).is_ok();
        if accepted {
            state.queued += n_bytes;
        }
        accepted
    }
}

impl Inbox {
    fn release(
        &self,
        frame: &Frame,
    ) {
        if is_bulk(frame.kind) {
            let mut state = self.budget.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.queued = state.queued.saturating_sub(frame.payload.len());
            self.budget.freed.notify_all();
        }
    }

    /// 프레임 하나를 기다려 꺼낸다. 연결이 끊겨 더 오지 않으면 오류다.
    pub fn recv(&self) -> Result<Frame, RecvError> {
        let frame = self.receiver.recv()?;
        self.release(&frame);
        Ok(frame)
    }

    pub fn recv_timeout(
        &self,
        wait: Duration,
    ) -> Result<Frame, RecvTimeoutError> {
        let frame = self.receiver.recv_timeout(wait)?;
        self.release(&frame);
        Ok(frame)
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        // 넣는 쪽이 상한 때문에 멈춰 있으면 풀어 주어 스레드가 끝나게 한다.
        let mut state = self.budget.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed = true;
        self.budget.freed.notify_all();
    }
}

/// 인사를 마친 데몬 연결. 받은 프레임은 `frames`에 쌓인다.
pub struct DaemonConnection {
    pub outbox: Outbox,
    pub frames: Inbox,
    pub daemon_pid: u32,
    pub daemon_fingerprint: String,
    /// 데몬이 인사에서 알린 프로토콜 버전. 이 필드를 모르던 옛 데몬(인사에 값이 없음)은 1이다.
    pub daemon_protocol_version: u32,
    /// 데몬 실행 파일이 앱이 기대하는 것과 다르지만(지문 불일치) 프로토콜은 같아 계속 쓰는 중이다. 데몬 쪽 수정은
    /// 이 데몬의 터미널이 모두 닫히고 새 데몬이 뜬 뒤에야 반영된다.
    pub outdated: bool,
    /// 연결한 데몬의 식별 글자(`DaemonInfo::identity`). 터미널 번호는 이 글자와 함께 쓴다.
    pub daemon_id: String,
    /// 앱이 데몬을 부모 작업 개체에서 분리하지 못했다. 앱을 닫으면 데몬과 터미널도 끝난다.
    pub breakaway_denied: bool,
}

impl DaemonConnection {
    /// `daemon.json`의 정보로 접속해 인사한다.
    ///
    /// 서버가 같은 사용자의 프로세스임을 확인한 뒤에야(`connect_verified`) 인사를 보낸다. 서버의 거부 문장은 그대로 오류가 되고,
    /// 시간 안에 답이 없으면 `TimedOut`이다.
    pub fn connect(info: &DaemonInfo) -> io::Result<Self> {
        let endpoint_name = info.endpoint.clone();
        let token = info.token.clone();
        // 연결·소유자 확인·인사 응답을 영원히 기다리는 서버에 붙잡히지 않도록 다른 스레드에서 하고 시간을 잰다.
        let (result_sender, result_receiver) = channel();
        thread::spawn(move || {
            let result = endpoint::connect_verified(&endpoint_name)
                .map_err(|error| match error {
                    PeerError::Io(error) => error,
                    other => io::Error::new(io::ErrorKind::PermissionDenied, other.to_string()),
                })
                .and_then(|stream| say_hello(stream, &token));
            let _ = result_sender.send(result);
        });
        let (stream, hello_ok) = result_receiver
            .recv_timeout(timeout::HANDSHAKE)
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Daemon did not answer"))??;

        let (mut receive_half, send_half) = stream.split();
        let (frame_sender, frames) = inbox_channel(MAX_QUEUED_BYTES);
        thread::spawn(move || {
            while let Ok(frame) = Frame::read_from(&mut receive_half) {
                if !frame_sender.push(frame) {
                    break;
                }
            }
        });
        Ok(Self {
            outbox: Outbox::spawn(send_half),
            frames,
            daemon_pid: hello_ok.pid,
            daemon_protocol_version: protocol_version_of(&hello_ok),
            daemon_fingerprint: hello_ok.fingerprint,
            outdated: false,
            daemon_id: info.identity(),
            breakaway_denied: info.breakaway_denied,
        })
    }

    pub fn send(
        &self,
        frame: &Frame,
    ) -> io::Result<()> {
        self.outbox.send(frame)
    }

    /// 이 연결을 더 쓰지 않는다고 데몬에 알린다(`BYE`). 데몬이 연결을 닫으면 받는 스레드도 끊김으로 끝난다.
    ///
    /// 터미널에 묶지 않고 버리는 연결(지문이 달라 쓰지 않는 데몬에 건 확인용 연결 등)은 이것으로 닫는다.
    /// 그냥 버리면 받는 스레드가 소켓을 쥔 채 남아 데몬이 연결 수 때문에 유휴 종료하지 못한다.
    pub fn close(self) {
        let _ = self.outbox.send(&Frame::new(kind::BYE, Vec::new()));
    }

    /// 지정한 종류 중 하나가 올 때까지 기다린다. 그 사이에 온 다른 프레임은 버린다. 시간이 지나거나 연결이 끊기면 None이다.
    pub fn wait_for_any(
        &self,
        kinds: &[u8],
        wait: Duration,
    ) -> Option<Frame> {
        let end = Instant::now() + wait;
        let mut found = None;
        while found.is_none() {
            match end.checked_duration_since(Instant::now()).map(|left| self.frames.recv_timeout(left)) {
                Some(Ok(frame)) if kinds.contains(&frame.kind) => found = Some(frame),
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            }
        }
        found
    }
}

/// 소유자를 확인한 연결로 인사를 보내고 응답을 읽는다. 서버가 인사를 거부하면 그 문장이 그대로 오류가 된다.
fn say_hello<S: Read + Write>(
    mut stream: S,
    token: &str,
) -> io::Result<(S, HelloOk)> {
    let hello = Hello { token: token.to_string(), protocol_version: PROTOCOL_VERSION };
    Frame::json(kind::HELLO, &hello).write_to(&mut stream)?;
    let reply = Frame::read_from(&mut stream)?;
    match reply.kind {
        kind::HELLO_OK => reply.parse_json::<HelloOk>().map(|hello_ok| (stream, hello_ok)),
        kind::ERR if reply.as_text() == HelloError::UnsupportedVersion.to_string() => {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, UnsupportedProtocol))
        }
        kind::ERR => Err(io::Error::new(io::ErrorKind::PermissionDenied, reply.as_text())),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected reply from daemon")),
    }
}

/// 데몬이 프로토콜 버전이 다르다는 이유로 인사를 거절했다. 인사의 버전을 따지고 인사 전용 연결을 모르던 옛 데몬(버전 1)의
/// 답이다. 다른 거절(토큰 오류 등)과 구별해야 이 데몬을 교체 대상으로 알릴 수 있다. 문장은 데몬이 보낸 그대로다.
#[derive(Debug)]
struct UnsupportedProtocol;

impl std::fmt::Display for UnsupportedProtocol {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&HelloError::UnsupportedVersion.to_string())
    }
}

impl std::error::Error for UnsupportedProtocol {}

/// 인사 오류가 프로토콜 버전을 이유로 한 거절인가.
fn is_unsupported_protocol(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<UnsupportedProtocol>())
}

/// 인사 답에 담긴 데몬의 프로토콜 버전. 값이 없던(0) 옛 데몬은 버전 1이다.
fn protocol_version_of(hello_ok: &HelloOk) -> u32 {
    if hello_ok.protocol_version == 0 { 1 } else { hello_ok.protocol_version }
}

/// 실행 중인 데몬이 앱의 것과 다를 때 어떻게 할지.
///
/// 데몬의 프로토콜 버전이 앱과 다르면 데몬을 쓸 수 없다. 프로토콜은 같고 실행 파일(지문)만 다르면 계속 쓸 수 있다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// 데몬을 절대 교체하지 않는다. 프로토콜이 다르면 쓰지 않고 알리며, 데몬과 터미널은 살아 있다.
    /// 앱을 쓰는 중 새 터미널에 연결할 때 쓴다 — 같은 앱의 다른 연결이 끊기면 안 된다.
    Keep,
    /// `Keep`에 더해, 프로토콜은 같고 지문만 다른 데몬은 터미널이 하나도 없을 때만 교체한다. 앱을 시작할 때 쓴다.
    ReplaceIfIdle,
    /// 프로토콜이 다른 데몬을 종료하고 새로 띄운다. 옛 데몬의 터미널은 사라진다. 사용자가 교체를 고른 뒤에 쓴다.
    /// 프로토콜은 같고 지문만 다른 데몬은 `ReplaceIfIdle`처럼 다룬다.
    Replace,
}

/// `ensure`의 결과.
pub enum Ensure {
    Connected(DaemonConnection),
    /// 실행 중인 데몬의 프로토콜 버전이 앱과 달라 사용자 결정이 필요하다. 데몬은 그대로 남아 있다.
    /// `daemon_version`이 None이면 인사를 거절한 옛 데몬(버전 1)이라 `Replace`가 프로세스를 강제로 끝낸다.
    ProtocolMismatch {
        pid: u32,
        daemon_version: Option<u32>,
    },
    /// 실행 중인 데몬이 연결은 받지만 인사에 답하지 않는다(멈춤). 사용자가 데몬을 다시 시작할지 정한다.
    Stalled {
        pid: u32,
    },
}

/// 실행 중인 데몬을 찾아보고 그 결과.
enum Existing {
    Ready(Ensure),
    /// 쓸 수 있는 데몬이 없다(처음이거나, 죽은 데몬의 흔적이거나, 방금 교체를 위해 종료시킴).
    Absent,
}

/// 데몬에 연결한다. 없으면 분리해서 띄운다.
///
/// `daemon_exe`는 앱이 짝으로 기대하는 데몬 실행 파일이다. 실행 중인 데몬은 프로토콜 버전을 먼저 비교하고(다르면
/// `ProtocolMismatch`로 알리거나 `Replace`가 교체한다), 같으면 지문을 비교한다. 지문만 다르면 데몬을 쓴다(`outdated`).
/// `ReplaceIfIdle`은 그 데몬에 터미널이 하나도 없을 때만 교체한다. 불일치는 앱 기록 파일에 남긴다.
/// 인사에 답하지 않는 데몬은 `Stalled`로 알리고 건드리지 않는다. 연결 실패(다른 사용자 소유 등)는 오류로 돌려주며,
/// 그 경우 새 데몬을 띄우지 않는다.
pub fn ensure(
    daemon_exe: &Path,
    policy: Policy,
) -> io::Result<Ensure> {
    let expected = fingerprint_of(daemon_exe)?;
    match connect_existing(daemon_exe, &expected, policy)? {
        Existing::Ready(ensure) => Ok(ensure),
        Existing::Absent => launch_and_connect(daemon_exe, &expected),
    }
}

/// 멈춘 데몬을 끝내고 새 데몬을 띄워 연결한다. 그 데몬의 터미널은 사라진다.
///
/// 멈춘 데몬은 종료 요청에도 답하지 못하므로 프로세스를 직접 끝낸다. `daemon.json`의 pid가 데몬 실행 파일의
/// 프로세스일 때만 끝낸다 — 그 번호를 다른 프로그램이 물려받았을 수 있기 때문이다.
pub fn restart_stalled_daemon(daemon_exe: &Path) -> io::Result<DaemonConnection> {
    let expected = fingerprint_of(daemon_exe)?;
    if let Some(info) = read_info() {
        app_log::event(&format!("restarting stalled daemon pid {}", info.pid));
        if info.is_running() {
            terminate_and_wait(&info, daemon_exe)?;
        }
        remove_info_owned_by(info.pid);
    }
    match launch_and_connect(daemon_exe, &expected)? {
        Ensure::Connected(connection) => Ok(connection),
        Ensure::ProtocolMismatch { .. } | Ensure::Stalled { .. } => {
            Err(io::Error::other("Another terminal daemon started first and cannot be used"))
        }
    }
}

/// 데몬 프로세스를 강제로 끝내고 사라지기를 기다린다. 사라지지 않으면 오류다 — 그대로 진행하면 `daemon.json`만 지워지고
/// 옛 데몬이 단일 실행 잠금을 쥔 채 남아 새 데몬이 뜨지 못한다.
fn terminate_and_wait(
    info: &DaemonInfo,
    daemon_exe: &Path,
) -> io::Result<()> {
    terminate_daemon(info.pid, daemon_exe)?;
    wait_until_gone(info);
    if info.is_running() { Err(io::Error::other("The old daemon did not exit")) } else { Ok(()) }
}

fn wait_until_gone(info: &DaemonInfo) {
    let end = Instant::now() + timeout::SHUTDOWN_EXIT;
    while info.is_running() && Instant::now() < end {
        thread::sleep(timeout::POLL);
    }
}

/// `daemon.json`의 데몬에 붙어 정책대로 쓸지·알릴지·교체할지 정한다.
///
/// 프로토콜 버전을 지문보다 먼저 본다 — 버전이 다른 데몬은 인사 전용 연결만 주므로 지문이 같아도 쓸 수 없다.
fn connect_existing(
    daemon_exe: &Path,
    expected_fingerprint: &str,
    policy: Policy,
) -> io::Result<Existing> {
    match read_info() {
        None => Ok(Existing::Absent),
        Some(info) if !info.is_running() => {
            app_log::event(&format!("stale daemon.json (pid {} is gone), removing", info.pid));
            remove_info_owned_by(info.pid);
            Ok(Existing::Absent)
        }
        Some(info) => match DaemonConnection::connect(&info) {
            Ok(connection) if connection.daemon_protocol_version != PROTOCOL_VERSION => {
                app_log::event(&format!(
                    "protocol mismatch: daemon v{} app v{PROTOCOL_VERSION} (pid {}), policy {policy:?}",
                    connection.daemon_protocol_version, connection.daemon_pid,
                ));
                match policy {
                    Policy::Replace => shut_down(&connection, &info).map(|()| Existing::Absent),
                    Policy::Keep | Policy::ReplaceIfIdle => {
                        let (pid, daemon_version) = (connection.daemon_pid, Some(connection.daemon_protocol_version));
                        connection.close();
                        Ok(Existing::Ready(Ensure::ProtocolMismatch { pid, daemon_version }))
                    }
                }
            }
            Ok(connection) if connection.daemon_fingerprint == expected_fingerprint => {
                Ok(Existing::Ready(Ensure::Connected(connection)))
            }
            Ok(mut connection) => {
                // 프로토콜은 같고 데몬 실행 파일만 다르다. 터미널이 하나도 없는 때만(시작할 때만) 바꾼다 — 터미널이 있으면
                // 그 셸을 끝내게 되고, 앱을 쓰는 중이면 같은 앱의 다른 연결이 끊긴다.
                let n_terminals = matches!(policy, Policy::ReplaceIfIdle | Policy::Replace)
                    .then(|| n_terminals_of(&connection))
                    .flatten();
                if n_terminals == Some(0) {
                    app_log::event(&format!(
                        "idle outdated daemon replaced: daemon {}.. app {}.. (pid {})",
                        short(&connection.daemon_fingerprint),
                        short(expected_fingerprint),
                        connection.daemon_pid,
                    ));
                    shut_down(&connection, &info).map(|()| Existing::Absent)
                } else {
                    app_log::event(&format!(
                        "outdated daemon kept: daemon {}.. app {}.. (pid {}){}",
                        short(&connection.daemon_fingerprint),
                        short(expected_fingerprint),
                        connection.daemon_pid,
                        n_terminals.map(|n| format!(", {n} terminals")).unwrap_or_default(),
                    ));
                    connection.outdated = true;
                    Ok(Existing::Ready(Ensure::Connected(connection)))
                }
            }
            // 인사의 버전을 따지는 옛 데몬(버전 1)은 우리 인사를 거절하고 연결을 닫아 종료 요청도 보낼 수 없다.
            // 교체를 고른 경우에만 프로세스를 직접 끝낸다(마지막 몇 초의 터미널 기록이 사라질 수 있다).
            Err(error) if is_unsupported_protocol(&error) => {
                app_log::event(&format!("daemon pid {} refused the hello: {error}, policy {policy:?}", info.pid));
                match policy {
                    Policy::Replace => {
                        app_log::event(&format!(
                            "force-stopping daemon pid {} (protocol v1 cannot be asked to exit)",
                            info.pid
                        ));
                        terminate_and_wait(&info, daemon_exe)?;
                        remove_info_of(&info);
                        Ok(Existing::Absent)
                    }
                    Policy::Keep | Policy::ReplaceIfIdle => {
                        Ok(Existing::Ready(Ensure::ProtocolMismatch { pid: info.pid, daemon_version: None }))
                    }
                }
            }
            // 파이프나 소켓이 없거나(`NotFound`) 소켓 파일만 남아 접속이 거부되는데(`ConnectionRefused`) 프로세스는 있다:
            // 죽은 데몬의 pid를 다른 프로세스가 물려받았다. 같은 데몬(pid와 시작 시각)의 정보일 때만 지운다.
            Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {
                app_log::event(&format!("daemon.json points to an unusable endpoint ({error}), removing"));
                remove_info_of(&info);
                Ok(Existing::Absent)
            }
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                app_log::event(&format!("daemon pid {} did not answer the hello", info.pid));
                Ok(Existing::Ready(Ensure::Stalled { pid: info.pid }))
            }
            Err(error) => {
                app_log::event(&format!("connect to existing daemon failed: {error}"));
                Err(error)
            }
        },
    }
}

fn short(fingerprint: &str) -> &str {
    fingerprint.get(..12).unwrap_or(fingerprint)
}

/// 데몬이 쥔 터미널 수. 시간 안에 답이 없으면 None이다.
fn n_terminals_of(connection: &DaemonConnection) -> Option<u32> {
    connection.send(&Frame::new(kind::STATUS_REQUEST, Vec::new())).ok()?;
    let reply = connection.wait_for_any(&[kind::STATUS], timeout::STATUS_REPLY)?;
    reply.parse_json::<Status>().ok().map(|status| status.n_terminals)
}

/// 이 연결로 데몬에 종료를 요청한다(모든 터미널의 셸이 끝나고 터미널 기록은 남는다). 요청이 데몬에 쓰이기를 `wait`까지 기다린다.
///
/// 앱이 곧 끝나므로 요청이 보내지기 전에 프로세스가 사라지지 않게 한다. 데몬이 종료를 마치기까지는 기다리지 않는다.
pub fn request_shutdown(
    outbox: &Outbox,
    wait: Duration,
) {
    let _ = outbox.send(&Frame::new(kind::SHUTDOWN, vec![]));
    outbox.flush(wait);
}

/// 옛 데몬에 종료를 요청하고 프로세스가 사라지기를 기다린다. 사라지지 않으면 오류다.
///
/// 요청이 쓰이지 않았거나 답이 없어도 프로세스가 사라지면 성공이다 — 다른 앱이 같은 데몬을 먼저 끝내는 중일 수 있다.
fn shut_down(
    connection: &DaemonConnection,
    info: &DaemonInfo,
) -> io::Result<()> {
    app_log::event(&format!("replacing daemon pid {}", info.pid));
    if connection.send(&Frame::new(kind::SHUTDOWN, vec![])).is_ok() {
        let _ = connection.wait_for_any(&[kind::STATUS], timeout::SHUTDOWN_REPLY);
    }
    wait_until_gone(info);
    if info.is_running() { Err(io::Error::other("The old daemon did not exit")) } else { Ok(()) }
}

/// 데몬을 띄우고 그 데몬이 `daemon.json`을 쓰면 연결한다. 시간 안에 뜨지 않으면 오류다.
///
/// 두 앱이 동시에 시작하면 데몬도 둘이 뜨지만 단일 실행 잠금을 진 쪽만 남는다. 진 데몬은 `AddrInUse`로 끝나므로,
/// `daemon.json`에 다른 데몬이 쓰여 있으면 그 데몬에 붙는다(`Policy::Keep`).
fn launch_and_connect(
    daemon_exe: &Path,
    expected_fingerprint: &str,
) -> io::Result<Ensure> {
    let started = Instant::now();
    // 데이터 폴더와 기록 폴더는 데몬을 띄우기 전에 앱이 만든다. 앱과 데몬이 같은 폴더의 접근 권한을
    // 동시에 처음 설정하면 서로 부딪쳐 데몬이 "액세스 거부"로 시작하지 못할 수 있다.
    ensure_private_dir(&native_dir().join("logs"))?;
    let launched = launch_daemon(daemon_exe)
        .map_err(|error| io::Error::new(error.kind(), format!("Could not start the terminal daemon: {error}")))?;
    let end = Instant::now() + timeout::DAEMON_START;
    let mut outcome: Option<io::Result<Ensure>> = None;
    let mut joined_other = false;
    while outcome.is_none() && Instant::now() < end {
        outcome = match read_info() {
            // 방금 띄운 데몬이 쓴 정보.
            Some(info) if info.pid == launched.pid => match DaemonConnection::connect(&info) {
                // 앱 옆 데몬 파일이 다른 프로토콜로 인사한다. 이 연결은 인사만 되므로 쓰지 않고 원인을 알린다 — 그대로 쓰면
                // 첫 요청이 거절되고, 다시 시도해도 같은 파일이 뜬다.
                Ok(connection) if connection.daemon_protocol_version != PROTOCOL_VERSION => {
                    let error = io::Error::other(format!(
                        "The daemon beside the app speaks protocol v{}; this Paddock needs v{PROTOCOL_VERSION}",
                        connection.daemon_protocol_version
                    ));
                    connection.close();
                    Some(Err(error))
                }
                Ok(connection) => Some(Ok(Ensure::Connected(connection))),
                Err(_) => None,
            },
            // 다른 데몬의 정보: 잠금을 다른 데몬이 먼저 얻었다. 죽은 데몬의 흔적은 아직 실행 중이 아니라서 건너뛴다.
            Some(info) if info.is_running() => match connect_existing(daemon_exe, expected_fingerprint, Policy::Keep) {
                Ok(Existing::Ready(ensure)) => {
                    joined_other = true;
                    Some(Ok(ensure))
                }
                Ok(Existing::Absent) | Err(_) => None,
            },
            _ => None,
        };
        if outcome.is_none() {
            thread::sleep(timeout::POLL);
        }
    }
    // 기록은 데몬이 준비된 뒤에 남긴다. 데몬이 시작하며 폴더 권한을 바꾸는 동안 앱이 기록 파일을 열어 두고 있으면
    // 권한 변경이 그 파일 때문에 실패할 수 있다.
    app_log::event(&format!(
        "launched daemon pid {} (breakaway denied: {}), ready: {}, {} ms{}",
        launched.pid,
        launched.breakaway_denied,
        matches!(outcome, Some(Ok(_))),
        started.elapsed().as_millis(),
        if joined_other { ", but another daemon holds the lock: connected to it" } else { "" },
    ));
    outcome.unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::TimedOut, "The terminal daemon did not start")))
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read, Write};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use paddock_protocol::daemon_info::read_info;
    use paddock_protocol::fingerprint::fingerprint_of;
    use paddock_protocol::frame::Frame;
    use paddock_protocol::message::{Status, kind};

    use super::*;
    use crate::terminal::session::{TerminalSession, Want, start_session};
    use crate::test_support::{DaemonCleanup, TempHome, altered_copy, daemon_exe};

    /// 쓴 내용을 기록하고 정해 둔 응답을 읽어 주는 가짜 연결.
    struct FakeStream {
        written: Arc<Mutex<Vec<u8>>>,
        reply: Cursor<Vec<u8>>,
    }

    impl Write for FakeStream {
        fn write(
            &mut self,
            buf: &[u8],
        ) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakeStream {
        fn read(
            &mut self,
            buf: &mut [u8],
        ) -> io::Result<usize> {
            self.reply.read(buf)
        }
    }

    fn reply_bytes(frame: &Frame) -> Vec<u8> {
        let mut bytes = Vec::new();
        frame.write_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn test_handshake_shows_server_error_text_as_is() {
        let reply = reply_bytes(&Frame::text(kind::ERR, "Bad token"));
        let written = Arc::new(Mutex::new(Vec::new()));
        let stream = FakeStream { written: written.clone(), reply: Cursor::new(reply) };
        let error = say_hello(stream, "token").err().expect("server error must surface");
        assert_eq!(error.to_string(), "Bad token");
        assert!(!written.lock().unwrap().is_empty(), "hello is sent to a same-user server");
    }

    fn connected(ensure: Ensure) -> DaemonConnection {
        match ensure {
            Ensure::Connected(connection) => connection,
            Ensure::ProtocolMismatch { .. } | Ensure::Stalled { .. } => panic!("expected a connection"),
        }
    }

    fn status_of(connection: &DaemonConnection) -> Status {
        connection.send(&Frame::new(kind::STATUS_REQUEST, vec![])).unwrap();
        connection.wait_for_any(&[kind::STATUS], Duration::from_secs(5)).expect("status reply").parse_json().unwrap()
    }

    #[test]
    fn test_handshake_names_the_protocol_version_refusal_apart_from_other_errors() {
        let refuse = |text: &str| {
            let stream =
                FakeStream { written: Arc::default(), reply: Cursor::new(reply_bytes(&Frame::text(kind::ERR, text))) };
            say_hello(stream, "token").err().expect("server error must surface")
        };
        let version = refuse("Unsupported protocol version");
        assert!(is_unsupported_protocol(&version));
        assert_eq!(version.to_string(), "Unsupported protocol version");
        assert!(!is_unsupported_protocol(&refuse("Bad token")));
    }

    #[test]
    fn test_a_hello_answer_without_a_protocol_version_means_version_1() {
        let answer = |protocol_version| HelloOk { pid: 1, fingerprint: "f".into(), protocol_version };
        assert_eq!(protocol_version_of(&answer(0)), 1);
        assert_eq!(protocol_version_of(&answer(2)), 2);
    }

    #[test]
    fn test_ensure_launches_daemon_then_reuses_it() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = connected(ensure(&exe, Policy::Keep).unwrap());
        let second = connected(ensure(&exe, Policy::Keep).unwrap());
        assert_eq!(first.daemon_pid, second.daemon_pid);
        assert_eq!(status_of(&first).pid, first.daemon_pid);
        assert_eq!(read_info().unwrap().pid, first.daemon_pid);
    }

    #[cfg(unix)]
    const SHELL: &str = "bash";
    #[cfg(windows)]
    const SHELL: &str = "cmd";

    /// 데몬에 터미널 하나를 연다. 돌려받은 세션을 쥐고 있는 동안 그 연결과 터미널이 산다.
    fn open_terminal(connection: DaemonConnection) -> TerminalSession {
        let want = Want::New { shell: SHELL.to_string(), cwd: None, wsl_distribution: None };
        start_session(connection, want, Instant::now()).unwrap()
    }

    /// 앱 기록 파일의 내용.
    fn app_log_text() -> String {
        std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs").join("app.log"))
            .unwrap_or_default()
    }

    /// 데몬을 띄우고 `daemon.json`이 써질 때까지만 기다린다(인사하지 않는다). 인사를 거절하는 데몬은 `ensure`로 띄울 수 없다.
    fn launch_without_greeting(exe: &Path) -> u32 {
        let launched = launch_daemon(exe).unwrap();
        let end = Instant::now() + Duration::from_secs(8);
        while !read_info().is_some_and(|info| info.pid == launched.pid) {
            assert!(Instant::now() < end, "the daemon did not write daemon.json");
            std::thread::sleep(Duration::from_millis(30));
        }
        launched.pid
    }

    #[test]
    fn test_c7_outdated_daemon_with_terminals_is_used_without_asking() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, Policy::Keep).unwrap());
        let old_pid = running.daemon_pid;
        let _terminal = open_terminal(running);

        // 앱만 업데이트돼 기대하는 데몬 파일의 지문이 실행 중인 데몬과 다르다. 터미널이 있으므로 묻지 않고 그 데몬을 쓴다.
        let altered = altered_copy(&exe, home.dir.path());
        let used = connected(ensure(&altered, Policy::ReplaceIfIdle).unwrap());
        assert_eq!(used.daemon_pid, old_pid);
        assert!(used.outdated);
        assert_eq!(used.daemon_fingerprint, fingerprint_of(&exe).unwrap());
        assert_eq!(status_of(&used).n_terminals, 1, "the terminal must still be alive");
        assert_eq!(read_info().unwrap().pid, old_pid);
        assert!(app_log_text().contains("outdated daemon kept"), "{}", app_log_text());
    }

    #[test]
    fn test_c7_outdated_daemon_with_no_terminals_is_replaced_at_start() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, Policy::Keep).unwrap());
        let old_pid = running.daemon_pid;
        let old_info = read_info().unwrap();
        running.close();

        let altered = altered_copy(&exe, home.dir.path());
        let replaced = connected(ensure(&altered, Policy::ReplaceIfIdle).unwrap());
        assert_ne!(replaced.daemon_pid, old_pid);
        assert_eq!(replaced.daemon_fingerprint, fingerprint_of(&altered).unwrap());
        assert!(!replaced.outdated);
        assert!(!old_info.is_running(), "the old daemon must be gone");
        assert_eq!(read_info().unwrap().pid, replaced.daemon_pid);
        assert!(app_log_text().contains("idle outdated daemon replaced"), "{}", app_log_text());
    }

    #[test]
    fn test_c7_outdated_daemon_is_kept_when_connecting_for_a_new_terminal() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, Policy::Keep).unwrap());
        let old_pid = running.daemon_pid;
        running.close();

        // 세션 중 새 터미널을 위한 연결은 터미널이 하나도 없어도 데몬을 바꾸지 않는다.
        let altered = altered_copy(&exe, home.dir.path());
        let used = connected(ensure(&altered, Policy::Keep).unwrap());
        assert_eq!(used.daemon_pid, old_pid);
        assert!(used.outdated);
        assert_eq!(read_info().unwrap().pid, old_pid);
    }

    #[test]
    fn test_c7_protocol_mismatch_asks_then_replace_shuts_the_old_daemon_down() {
        let mut home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        // 프로토콜 버전 1이라고 인사하는 데몬(지문은 앱과 같다 — 앱은 지문보다 버전을 먼저 본다).
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", Some("1"));
        let old_pid = launch_without_greeting(&exe);
        let old_info = read_info().unwrap();
        // 새로 뜨는 데몬은 훅 없이 실제 버전으로 인사해야 한다.
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", None);

        for policy in [Policy::Keep, Policy::ReplaceIfIdle] {
            match ensure(&exe, policy).unwrap() {
                Ensure::ProtocolMismatch { pid, daemon_version } => {
                    assert_eq!((pid, daemon_version), (old_pid, Some(1)))
                }
                Ensure::Connected(_) | Ensure::Stalled { .. } => {
                    panic!("a daemon with another protocol must not be used")
                }
            }
            assert!(old_info.is_running(), "{policy:?} must leave the daemon running");
        }

        let replaced = connected(ensure(&exe, Policy::Replace).unwrap());
        assert_ne!(replaced.daemon_pid, old_pid);
        assert_eq!(replaced.daemon_protocol_version, PROTOCOL_VERSION);
        assert!(!old_info.is_running(), "the old daemon must be gone");
        assert_eq!(read_info().unwrap().pid, replaced.daemon_pid);
        // 종료 요청이 닿는 데몬이라 프로세스를 강제로 끝내지 않는다.
        assert!(!app_log_text().contains("force-stopping"), "{}", app_log_text());
    }

    #[test]
    fn test_launching_a_daemon_that_speaks_another_protocol_fails_with_the_cause() {
        let mut home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        // 앱 옆의 데몬 파일이 다른 프로토콜로 인사한다. 연결해 쓰면 첫 요청이 ERR가 되어 같은 파일을 다시 띄우는 일이 반복된다.
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", Some("1"));
        let error = ensure(&exe, Policy::ReplaceIfIdle).err().expect("a daemon of another protocol must not be used");
        assert_eq!(
            error.to_string(),
            format!("The daemon beside the app speaks protocol v1; this Paddock needs v{PROTOCOL_VERSION}")
        );
        assert!(app_log_text().contains("ready: false"), "{}", app_log_text());
    }

    #[test]
    fn test_c7_replace_falls_back_to_terminate_when_the_old_daemon_rejects_the_hello() {
        let mut home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        // 버전이 다른 인사를 ERR로 거절하고 연결을 닫는 데몬(인사의 버전을 따지던 옛 v1 데몬).
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", Some("1"));
        home.set_env("PADDOCK_DAEMON_STRICT_HELLO", Some("1"));
        let old_pid = launch_without_greeting(&exe);
        let old_info = read_info().unwrap();
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", None);
        home.set_env("PADDOCK_DAEMON_STRICT_HELLO", None);

        for policy in [Policy::Keep, Policy::ReplaceIfIdle] {
            match ensure(&exe, policy).unwrap() {
                Ensure::ProtocolMismatch { pid, daemon_version } => assert_eq!((pid, daemon_version), (old_pid, None)),
                Ensure::Connected(_) | Ensure::Stalled { .. } => panic!("a rejecting daemon must not be used"),
            }
            assert!(old_info.is_running(), "{policy:?} must leave the daemon running");
        }

        let replaced = connected(ensure(&exe, Policy::Replace).unwrap());
        assert_ne!(replaced.daemon_pid, old_pid);
        assert_eq!(replaced.daemon_protocol_version, PROTOCOL_VERSION);
        assert!(!old_info.is_running(), "the old daemon must be gone");
        assert_eq!(read_info().unwrap().pid, replaced.daemon_pid);
        assert!(app_log_text().contains("force-stopping"), "{}", app_log_text());
    }

    /// 두 앱이 동시에 시작해도 데몬이 하나만 남는다. 경쟁을 재현한다는 보장은 없어 회귀 방지용이다.
    #[test]
    fn test_two_apps_starting_at_once_end_up_on_one_daemon() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| connected(ensure(&exe, Policy::ReplaceIfIdle).unwrap()));
            let second = scope.spawn(|| connected(ensure(&exe, Policy::ReplaceIfIdle).unwrap()));
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(first.daemon_pid, second.daemon_pid);
        assert_eq!(read_info().unwrap().pid, first.daemon_pid);
        assert_eq!(status_of(&first).pid, first.daemon_pid);
        assert_eq!(status_of(&second).pid, first.daemon_pid);
        // 잠금에 진 데몬이 모두 끝나기를 기다린다. 정리가 이긴 데몬을 먼저 끝내면 아직 잠금을 기다리던 진 데몬이 이어받아 남는다.
        let n_launched = app_log_text().matches("launched daemon pid").count();
        let end = Instant::now() + Duration::from_secs(6);
        let n_lost = || {
            let log =
                std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs").join("daemon.log"));
            log.unwrap_or_default().matches("another daemon is already running").count()
        };
        while n_lost() + 1 < n_launched && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(n_lost() + 1, n_launched, "every daemon but one must have lost the lock and exited");
    }

    /// 데몬 기록 파일에서 "connection N closed" 줄 수.
    fn n_closed_connections() -> usize {
        let log = std::fs::read_to_string(paddock_protocol::daemon_info::native_dir().join("logs").join("daemon.log"));
        log.unwrap_or_default().lines().filter(|line| line.contains(" closed")).count()
    }

    fn wait_for_closed_connections(n_expected: usize) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while n_closed_connections() < n_expected && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(30));
        }
        n_closed_connections() >= n_expected
    }

    #[test]
    fn test_mismatch_probe_connection_is_closed_when_the_app_gives_up_on_it() {
        let mut home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        home.set_env("PADDOCK_DAEMON_PROTOCOL_VERSION", Some("1"));
        launch_without_greeting(&exe);
        let n_closed_before = n_closed_connections();
        // 프로토콜이 달라 쓰지 않는 데몬에 건 확인용 연결이 열려 있으면, 데몬은 그 연결 때문에 유휴 종료하지 못한다.
        assert!(matches!(ensure(&exe, Policy::Keep).unwrap(), Ensure::ProtocolMismatch { .. }));
        assert!(wait_for_closed_connections(n_closed_before + 1), "the probe connection stayed open");
    }

    #[test]
    fn test_c_d13_inbox_blocks_the_reader_until_the_consumer_frees_bytes() {
        let (sender, inbox) = inbox_channel(100);
        assert!(sender.push(Frame::new(kind::OUTPUT, vec![0; 60])));
        // 둘째 프레임은 합이 상한을 넘으므로 소비가 일어날 때까지 읽는 쪽(밀어 넣는 쪽)이 멈춘다.
        let blocked = std::thread::spawn(move || {
            sender.push(Frame::new(kind::OUTPUT, vec![0; 60]));
            sender
        });
        std::thread::sleep(Duration::from_millis(150));
        assert!(!blocked.is_finished(), "reader must wait while the queue is over its byte limit");
        assert_eq!(inbox.recv().unwrap().payload.len(), 60);
        let _sender = blocked.join().unwrap();
        assert_eq!(inbox.recv().unwrap().payload.len(), 60);
    }

    #[test]
    fn test_inbox_control_frames_and_one_oversized_frame_never_deadlock() {
        let (sender, inbox) = inbox_channel(100);
        // 상한보다 큰 프레임도 큐가 비어 있으면 들어온다.
        assert!(sender.push(Frame::new(kind::REPLAY, vec![0; 500])));
        // 제어 프레임은 큐가 가득 차 있어도 막히지 않는다.
        assert!(sender.push(Frame::new(kind::ATTACHED, vec![1])));
        assert_eq!(inbox.recv().unwrap().kind, kind::REPLAY);
        assert_eq!(inbox.recv().unwrap().kind, kind::ATTACHED);
    }

    /// 첫 쓰기가 `gate`가 열릴 때까지 멈추고, 쓴 바이트를 모으는 느린 상대.
    struct GatedWriter {
        gate: std::sync::mpsc::Receiver<()>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for GatedWriter {
        fn write(
            &mut self,
            buf: &[u8],
        ) -> io::Result<usize> {
            let _ = self.gate.recv();
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_outbox_send_returns_before_the_peer_reads_and_flush_waits_for_the_write() {
        let (open_gate, gate) = std::sync::mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let outbox = Outbox::spawn(GatedWriter { gate, written: written.clone() });
        let started = Instant::now();
        outbox.send(&Frame::new(kind::INPUT, b"x".to_vec())).unwrap();
        assert!(started.elapsed() < Duration::from_millis(100), "send must not wait for the peer");
        assert!(written.lock().unwrap().is_empty());
        // 상대가 읽기 시작하면(문이 열리면) 쓰기가 끝나고 flush가 돌아온다. 프레임 머리 쓰기와 내용 쓰기를 모두 연다.
        for _ in 0..4 {
            let _ = open_gate.send(());
        }
        assert!(outbox.flush(Duration::from_secs(2)));
        assert!(!written.lock().unwrap().is_empty());
    }

    #[test]
    fn test_connection_reports_breakaway_denied_from_daemon_json() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = connected(ensure(&exe, Policy::Keep).unwrap());
        assert!(!first.breakaway_denied);
        let mut info = read_info().unwrap();
        info.breakaway_denied = true;
        paddock_protocol::daemon_info::write_info(&info).unwrap();
        assert!(connected(ensure(&exe, Policy::Keep).unwrap()).breakaway_denied);
    }

    #[cfg(unix)]
    #[test]
    fn test_stalled_daemon_is_reported_and_can_be_restarted() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, Policy::Keep).unwrap());
        // 멈춘 데몬: 연결은 받지만 인사에 답하지 못한다.
        unsafe { libc::kill(running.daemon_pid as i32, libc::SIGSTOP) };
        let outcome = ensure(&exe, Policy::Keep).unwrap();
        assert!(matches!(outcome, Ensure::Stalled { pid } if pid == running.daemon_pid));
        let restarted = restart_stalled_daemon(&exe).unwrap();
        assert_ne!(restarted.daemon_pid, running.daemon_pid);
        assert_eq!(status_of(&restarted).pid, restarted.daemon_pid);
    }

    /// 죽은 데몬의 pid를 다른 프로세스가 물려받고 소켓 파일만 남았을 때: 접속은 거부되고 `NotFound`가 아니다.
    #[cfg(unix)]
    #[test]
    fn test_refused_socket_of_a_reused_pid_is_treated_as_no_daemon() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let endpoint = paddock_protocol::endpoint::endpoint_name().unwrap();
        let socket_dir = Path::new(&endpoint).parent().unwrap();
        paddock_protocol::daemon_info::ensure_private_dir(socket_dir).unwrap();
        // 리스너를 버려도 소켓 파일은 남는다. 아무도 받지 않는 소켓이라 접속은 거부된다.
        drop(std::os::unix::net::UnixListener::bind(&endpoint).unwrap());
        // pid를 물려받은 다른 프로그램 역할의 살아 있는 프로세스.
        let mut other_program = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        let stale = paddock_protocol::daemon_info::DaemonInfo {
            pid: other_program.id(),
            endpoint,
            token: "stale".into(),
            fingerprint: "stale".into(),
            started: "2026-01-01T00:00:00Z".into(),
            breakaway_denied: false,
        };
        paddock_protocol::daemon_info::write_info(&stale).unwrap();

        let connection = connected(ensure(&exe, Policy::Keep).unwrap());
        assert_ne!(connection.daemon_pid, stale.pid, "a new daemon must be started");
        assert_ne!(read_info().unwrap().identity(), stale.identity());
        other_program.kill().unwrap();
        other_program.wait().unwrap();
    }
}
