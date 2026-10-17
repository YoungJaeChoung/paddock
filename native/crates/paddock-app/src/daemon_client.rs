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
use paddock_protocol::message::{Hello, HelloOk, PROTOCOL_VERSION, kind};
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
            daemon_fingerprint: hello_ok.fingerprint,
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
        kind::ERR => Err(io::Error::new(io::ErrorKind::PermissionDenied, reply.as_text())),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected reply from daemon")),
    }
}

/// 앱의 데몬 지문이 실행 중인 데몬과 다를 때 어떻게 할지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MismatchPolicy {
    /// 데몬을 그대로 두고 쓰지 않는다. 데몬과 터미널은 살아 있다.
    Quit,
    /// 옛 데몬에 종료를 요청하고 새로 띄운다. 옛 데몬의 터미널은 사라진다.
    Replace,
}

/// `ensure`의 결과.
pub enum Ensure {
    Connected(DaemonConnection),
    /// 실행 중인 데몬의 지문이 앱이 기대하는 것과 달라 사용자 결정이 필요하다. 데몬은 그대로 남아 있다.
    MismatchKept {
        daemon_fingerprint: String,
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
/// `daemon_exe`는 앱이 짝으로 기대하는 데몬 실행 파일이다. 실행 중인 데몬의 지문이 이 파일의 지문과 다르면
/// `policy`에 따라 데몬을 그대로 두거나(`MismatchKept`) 교체한다. 지문 불일치는 앱 기록 파일에 남긴다.
/// 인사에 답하지 않는 데몬은 `Stalled`로 알리고 건드리지 않는다. 연결 실패(다른 사용자 소유 등)는 오류로 돌려주며,
/// 그 경우 새 데몬을 띄우지 않는다.
pub fn ensure(
    daemon_exe: &Path,
    policy: MismatchPolicy,
) -> io::Result<Ensure> {
    let expected = fingerprint_of(daemon_exe)?;
    match connect_existing(&expected, policy)? {
        Existing::Ready(ensure) => Ok(ensure),
        Existing::Absent => launch_and_connect(daemon_exe).map(Ensure::Connected),
    }
}

/// 멈춘 데몬을 끝내고 새 데몬을 띄워 연결한다. 그 데몬의 터미널은 사라진다.
///
/// 멈춘 데몬은 종료 요청에도 답하지 못하므로 프로세스를 직접 끝낸다. `daemon.json`의 pid가 데몬 실행 파일의
/// 프로세스일 때만 끝낸다 — 그 번호를 다른 프로그램이 물려받았을 수 있기 때문이다.
pub fn restart_stalled_daemon(daemon_exe: &Path) -> io::Result<DaemonConnection> {
    if let Some(info) = read_info() {
        app_log::event(&format!("restarting stalled daemon pid {}", info.pid));
        if info.is_running() {
            terminate_daemon(info.pid, daemon_exe)?;
            wait_until_gone(&info);
        }
        remove_info_owned_by(info.pid);
    }
    launch_and_connect(daemon_exe)
}

fn wait_until_gone(info: &DaemonInfo) {
    let end = Instant::now() + timeout::SHUTDOWN_EXIT;
    while info.is_running() && Instant::now() < end {
        thread::sleep(timeout::POLL);
    }
}

fn connect_existing(
    expected_fingerprint: &str,
    policy: MismatchPolicy,
) -> io::Result<Existing> {
    match read_info() {
        None => Ok(Existing::Absent),
        Some(info) if !info.is_running() => {
            app_log::event(&format!("stale daemon.json (pid {} is gone), removing", info.pid));
            remove_info_owned_by(info.pid);
            Ok(Existing::Absent)
        }
        Some(info) => match DaemonConnection::connect(&info) {
            Ok(connection) if connection.daemon_fingerprint == expected_fingerprint => {
                Ok(Existing::Ready(Ensure::Connected(connection)))
            }
            Ok(connection) => {
                app_log::event(&format!(
                    "fingerprint mismatch: daemon {}.. app {}.. (pid {}), policy {policy:?}",
                    short(&connection.daemon_fingerprint),
                    short(expected_fingerprint),
                    connection.daemon_pid,
                ));
                match policy {
                    MismatchPolicy::Quit => {
                        let daemon_fingerprint = connection.daemon_fingerprint.clone();
                        connection.close();
                        Ok(Existing::Ready(Ensure::MismatchKept { daemon_fingerprint }))
                    }
                    MismatchPolicy::Replace => shut_down(&connection, &info).map(|()| Existing::Absent),
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
fn shut_down(
    connection: &DaemonConnection,
    info: &DaemonInfo,
) -> io::Result<()> {
    app_log::event(&format!("replacing daemon pid {}", info.pid));
    connection.send(&Frame::new(kind::SHUTDOWN, vec![]))?;
    let _ = connection.wait_for_any(&[kind::STATUS], timeout::SHUTDOWN_REPLY);
    wait_until_gone(info);
    if info.is_running() { Err(io::Error::other("The old daemon did not exit")) } else { Ok(()) }
}

/// 데몬을 띄우고 그 데몬이 `daemon.json`을 쓰면 연결한다. 시간 안에 뜨지 않으면 오류다.
fn launch_and_connect(daemon_exe: &Path) -> io::Result<DaemonConnection> {
    let started = Instant::now();
    // 데이터 폴더와 기록 폴더는 데몬을 띄우기 전에 앱이 만든다. 앱과 데몬이 같은 폴더의 접근 권한을
    // 동시에 처음 설정하면 서로 부딪쳐 데몬이 "액세스 거부"로 시작하지 못할 수 있다.
    ensure_private_dir(&native_dir().join("logs"))?;
    let launched = launch_daemon(daemon_exe)
        .map_err(|error| io::Error::new(error.kind(), format!("Could not start the terminal daemon: {error}")))?;
    let end = Instant::now() + timeout::DAEMON_START;
    let mut connection = None;
    while connection.is_none() && Instant::now() < end {
        // 옛 데몬의 흔적이 아니라 방금 띄운 데몬이 쓴 정보일 때만 접속한다.
        connection =
            read_info().filter(|info| info.pid == launched.pid).and_then(|info| DaemonConnection::connect(&info).ok());
        if connection.is_none() {
            thread::sleep(timeout::POLL);
        }
    }
    // 기록은 데몬이 준비된 뒤에 남긴다. 데몬이 시작하며 폴더 권한을 바꾸는 동안 앱이 기록 파일을 열어 두고 있으면
    // 권한 변경이 그 파일 때문에 실패할 수 있다.
    app_log::event(&format!(
        "launched daemon pid {} (breakaway denied: {}), ready: {}, {} ms",
        launched.pid,
        launched.breakaway_denied,
        connection.is_some(),
        started.elapsed().as_millis()
    ));
    connection.ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "The terminal daemon did not start"))
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
    use crate::test_support::{DaemonCleanup, TempHome, daemon_exe};

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

    /// 실행 파일 뒤에 한 바이트를 덧붙인 사본. 그대로 실행되지만 지문이 다르다.
    fn altered_copy(
        exe: &Path,
        into: &Path,
    ) -> std::path::PathBuf {
        let copy = into.join(format!("paddock-daemon-altered{}", std::env::consts::EXE_SUFFIX));
        let mut bytes = std::fs::read(exe).unwrap();
        bytes.push(0);
        std::fs::write(&copy, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        copy
    }

    fn connected(ensure: Ensure) -> DaemonConnection {
        match ensure {
            Ensure::Connected(connection) => connection,
            Ensure::MismatchKept { .. } | Ensure::Stalled { .. } => panic!("expected a connection"),
        }
    }

    fn status_of(connection: &DaemonConnection) -> Status {
        connection.send(&Frame::new(kind::STATUS_REQUEST, vec![])).unwrap();
        connection.wait_for_any(&[kind::STATUS], Duration::from_secs(5)).expect("status reply").parse_json().unwrap()
    }

    #[test]
    fn test_ensure_launches_daemon_then_reuses_it() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let first = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        let second = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        assert_eq!(first.daemon_pid, second.daemon_pid);
        assert_eq!(status_of(&first).pid, first.daemon_pid);
        assert_eq!(read_info().unwrap().pid, first.daemon_pid);
    }

    #[test]
    fn test_c7_fingerprint_mismatch_quit_keeps_daemon() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        let old_fingerprint = fingerprint_of(&exe).unwrap();

        // 앱이 기대하는 데몬 파일이 실행 중인 데몬과 달라진 상황(앱만 업데이트됨).
        let altered = altered_copy(&exe, home.dir.path());
        match ensure(&altered, MismatchPolicy::Quit).unwrap() {
            Ensure::MismatchKept { daemon_fingerprint } => assert_eq!(daemon_fingerprint, old_fingerprint),
            Ensure::Connected(_) | Ensure::Stalled { .. } => panic!("a mismatched daemon must not be used"),
        }
        // Quit을 골라도 데몬과 그 터미널은 그대로다.
        assert_eq!(status_of(&running).pid, running.daemon_pid);
        assert_eq!(read_info().unwrap().pid, running.daemon_pid);
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
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let _running = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        let n_closed_before = n_closed_connections();
        // 지문이 달라 쓰지 않는 데몬에 건 확인용 연결이 열려 있으면, 데몬은 그 연결 때문에 유휴 종료하지 못한다.
        let altered = altered_copy(&exe, home.dir.path());
        assert!(matches!(ensure(&altered, MismatchPolicy::Quit).unwrap(), Ensure::MismatchKept { .. }));
        assert!(wait_for_closed_connections(n_closed_before + 1), "the probe connection stayed open");
    }

    #[test]
    fn test_c7_fingerprint_mismatch_replace_restarts_daemon() {
        let home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let old = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());

        let altered = altered_copy(&exe, home.dir.path());
        let replaced = connected(ensure(&altered, MismatchPolicy::Replace).unwrap());
        assert_ne!(replaced.daemon_pid, old.daemon_pid);
        assert_eq!(replaced.daemon_fingerprint, fingerprint_of(&altered).unwrap());
        let end = Instant::now() + Duration::from_secs(5);
        while paddock_protocol::daemon_info::read_info().is_some_and(|info| info.pid == old.daemon_pid)
            && Instant::now() < end
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(read_info().unwrap().pid, replaced.daemon_pid);
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
        let first = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        assert!(!first.breakaway_denied);
        let mut info = read_info().unwrap();
        info.breakaway_denied = true;
        paddock_protocol::daemon_info::write_info(&info).unwrap();
        assert!(connected(ensure(&exe, MismatchPolicy::Quit).unwrap()).breakaway_denied);
    }

    #[cfg(unix)]
    #[test]
    fn test_stalled_daemon_is_reported_and_can_be_restarted() {
        let _home = TempHome::new();
        let _cleanup = DaemonCleanup;
        let exe = daemon_exe();
        let running = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        // 멈춘 데몬: 연결은 받지만 인사에 답하지 못한다.
        unsafe { libc::kill(running.daemon_pid as i32, libc::SIGSTOP) };
        let outcome = ensure(&exe, MismatchPolicy::Quit).unwrap();
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

        let connection = connected(ensure(&exe, MismatchPolicy::Quit).unwrap());
        assert_ne!(connection.daemon_pid, stale.pid, "a new daemon must be started");
        assert_ne!(read_info().unwrap().identity(), stale.identity());
        other_program.kill().unwrap();
        other_program.wait().unwrap();
    }
}
