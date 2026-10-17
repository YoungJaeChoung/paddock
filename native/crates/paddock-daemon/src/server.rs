//! 연결 하나를 맡는 서버 쪽 처리: 접속자 확인 → 인사(hello) → 명령 분기.
use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{RecvHalf, Stream};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Attach, Hello, HelloOk, Kill, List, New, PROTOCOL_VERSION, Resize, Status, kind};
use paddock_protocol::peer::{PeerCredentials, PeerRole, UserId, require_same_user};

use crate::log;
use crate::outbox::{self, Outbox};
use crate::terminal::{Registry, Terminal};

/// 모든 연결이 함께 보는 데몬의 고정 정보.
pub struct Context {
    pub registry: Arc<Registry>,
    pub token: String,
    pub fingerprint: String,
    pub own_user: UserId,
    pub endpoint: String,
}

/// 접속자가 데몬과 같은 사용자일 때만 연결을 돌려준다. 다르거나 확인하지 못하면 한 바이트도 보내지 않고 연결을 버린다.
pub fn admit<S: PeerCredentials>(
    stream: S,
    own: &UserId,
) -> Option<S> {
    match require_same_user(stream, PeerRole::Client, own) {
        Ok(stream) => Some(stream),
        Err(error) => {
            log::event(&format!("connection refused: {error}"));
            None
        }
    }
}

/// 데몬 접속 정보와 소켓 파일을 지우고 프로세스를 끝낸다. 유휴 종료와 종료 요청이 함께 쓴다.
pub fn exit_daemon(ctx: &Context) -> ! {
    paddock_protocol::daemon_info::remove_info_owned_by(std::process::id());
    // Windows의 명명된 파이프는 프로세스가 끝나면 사라지고, Unix의 소켓은 파일이라 직접 지운다.
    #[cfg(unix)]
    let _ = std::fs::remove_file(&ctx.endpoint);
    #[cfg(windows)]
    let _ = &ctx.endpoint;
    std::process::exit(0)
}

/// 연결 하나를 끝까지 처리한다. 연결이 끊기거나 인사가 틀리면 돌아온다.
///
/// 인사를 마친 연결은 열려 있는 동안 레지스트리에 연결로 세어져, 터미널이 없어도 데몬이 유휴 종료하지 않게 붙든다.
///
/// 한 연결은 한 번에 터미널 하나만 다룬다. `NEW`나 `ATTACH`가 성공하면 그 터미널이 연결의 대상이 되고,
/// `INPUT`·`RESIZE`는 그 대상에게 간다.
pub fn serve(
    stream: Stream,
    conn_id: u64,
    ctx: &Context,
) {
    if let Some(stream) = admit(stream, &ctx.own_user) {
        let (mut recv, mut send) = stream.split();
        let (outbox, inbox) = outbox::channel();
        std::thread::spawn(move || inbox.run(|frame| frame.write_to(&mut send)));
        if greet(&mut recv, &outbox, ctx, conn_id) {
            let _connection = ctx.registry.connection_opened();
            let mut session = Session { conn_id, outbox, attached: None };
            while let Ok(frame) = Frame::read_from(&mut recv) {
                if session.dispatch(frame, ctx) == Flow::Close {
                    break;
                }
            }
            session.close(ctx);
        }
        log::event(&format!("connection {conn_id} closed"));
    }
}

/// 첫 프레임이 토큰·버전이 맞는 `HELLO`인지 보고 답한다. 맞으면 `true`.
///
/// 틀리면 오류 프레임을 한 번 보낸 뒤 호출자가 연결을 닫는다. `HELLO`가 아닌 첫 프레임은 답 없이 닫는다.
fn greet(
    recv: &mut RecvHalf,
    outbox: &Outbox,
    ctx: &Context,
    conn_id: u64,
) -> bool {
    let first = Frame::read_from(recv).ok().filter(|frame| frame.kind == kind::HELLO);
    let hello = first.as_ref().and_then(|frame| frame.parse_json::<Hello>().ok());
    let mut accepted = false;
    match hello.map(|hello| hello.check(&ctx.token, PROTOCOL_VERSION)) {
        None => log::event(&format!("connection {conn_id}: no hello, dropped")),
        Some(Err(error)) => {
            log::event(&format!("connection {conn_id}: {error}, dropped"));
            outbox.send(Frame::text(kind::ERR, &error.to_string()));
        }
        Some(Ok(())) => {
            let ok = HelloOk { pid: std::process::id(), fingerprint: ctx.fingerprint.clone() };
            outbox.send(Frame::json(kind::HELLO_OK, &ok));
            accepted = true;
        }
    }
    accepted
}

/// 요청 하나를 처리한 뒤 연결을 어떻게 할지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    /// 다음 요청을 계속 받는다.
    Continue,
    /// 연결을 닫는다. 앱의 읽기 스레드는 끊김으로 끝난다.
    Close,
}

/// 인사를 마친 연결의 상태.
struct Session {
    conn_id: u64,
    outbox: Outbox,
    attached: Option<Arc<Terminal>>,
}

impl Session {
    fn dispatch(
        &mut self,
        frame: Frame,
        ctx: &Context,
    ) -> Flow {
        let mut flow = Flow::Continue;
        match frame.kind {
            kind::NEW => self.on_new(&frame, ctx),
            kind::ATTACH => self.on_attach(&frame, ctx),
            kind::INPUT => {
                if let Some(terminal) = &self.attached {
                    terminal.input(frame.payload);
                }
            }
            kind::RESIZE => {
                if let (Some(terminal), Ok(resize)) = (&self.attached, frame.parse_json::<Resize>()) {
                    terminal.resize(resize.cols, resize.rows);
                }
            }
            kind::LIST => self.reply_list(ctx),
            kind::KILL => flow = self.on_kill(&frame, ctx),
            kind::BYE => flow = Flow::Close,
            kind::STATUS_REQUEST => {
                let status = Status { pid: std::process::id(), n_terminals: ctx.registry.len() as u32 };
                self.outbox.send(Frame::json(kind::STATUS, &status));
            }
            kind::SHUTDOWN => self.on_shutdown(ctx),
            _ => {}
        }
        flow
    }

    fn reply_error(
        &self,
        text: &str,
    ) {
        self.outbox.send(Frame::text(kind::ERR, text));
    }

    fn reply_list(
        &self,
        ctx: &Context,
    ) {
        let list = List { terminals: ctx.registry.list() };
        self.outbox.send(Frame::json(kind::LIST_REPLY, &list));
    }

    /// 연결의 대상을 `terminal`로 바꾸고 붙는다. 이전 대상이 있으면 그쪽 수신자는 비운다.
    fn attach_to(
        &mut self,
        terminal: Arc<Terminal>,
        cols: u16,
        rows: u16,
        ctx: &Context,
    ) {
        self.release(ctx);
        terminal.attach(self.conn_id, &self.outbox, cols, rows);
        self.attached = Some(terminal);
    }

    /// 대상 터미널에서 이 연결을 떼고, 셸이 이미 끝난 터미널이면 목록에서 뺀다.
    fn release(
        &mut self,
        ctx: &Context,
    ) {
        if let Some(terminal) = self.attached.take()
            && terminal.detach(self.conn_id)
        {
            ctx.registry.remove(terminal.id);
        }
    }

    fn on_new(
        &mut self,
        frame: &Frame,
        ctx: &Context,
    ) {
        match frame.parse_json::<New>() {
            Ok(request) => match Terminal::spawn(&ctx.registry, &request) {
                Ok(terminal) => self.attach_to(terminal, request.cols, request.rows, ctx),
                Err(error) => {
                    // 오류 문장에는 폴더 경로가 들어 있을 수 있어 로그에는 종류만 남긴다(사용자 화면 안내에는 그대로 보인다).
                    log::event(&format!("terminal start failed (shell {}): {:?}", request.shell, error.kind()));
                    self.reply_error(&format!("Could not start terminal: {error}"));
                }
            },
            Err(_) => self.reply_error("Malformed request"),
        }
    }

    fn on_attach(
        &mut self,
        frame: &Frame,
        ctx: &Context,
    ) {
        match frame.parse_json::<Attach>() {
            Ok(request) => match ctx.registry.get(request.id) {
                Some(terminal) => self.attach_to(terminal, request.cols, request.rows, ctx),
                None => self.reply_error("No such terminal"),
            },
            Err(_) => self.reply_error("Malformed request"),
        }
    }

    /// 터미널을 끝내고 목록을 답한다. 이 연결이 묶인 터미널이면 연결도 닫는다 — 앱은 닫은 탭의 연결을 더 쓰지 않는데,
    /// 열려 있으면 연결 수가 남아 데몬이 유휴 종료하지 못하고 앱의 읽기 스레드도 앱이 끝날 때까지 남는다.
    fn on_kill(
        &self,
        frame: &Frame,
        ctx: &Context,
    ) -> Flow {
        let mut flow = Flow::Continue;
        match frame.parse_json::<Kill>() {
            Ok(request) => match ctx.registry.remove(request.id) {
                Some(terminal) => {
                    // 사용자가 끝낸 터미널은 되살릴 것이 아니다.
                    ctx.registry.history().remove(terminal.id);
                    terminal.kill();
                    self.reply_list(ctx);
                    if self.attached.as_ref().is_some_and(|attached| attached.id == request.id) {
                        flow = Flow::Close;
                    }
                }
                None => self.reply_error("No such terminal"),
            },
            Err(_) => self.reply_error("Malformed request"),
        }
        flow
    }

    /// 모든 터미널을 끝내고 데몬을 종료한다. 답을 보낸 뒤 셸이 정리될 시간을 조금 두고 나간다.
    fn on_shutdown(
        &self,
        ctx: &Context,
    ) -> ! {
        log::event("shutdown requested: killing terminals and exiting");
        // 셸이 끝나기 전에 마지막 화면을 내려써야 다시 열 때 되살린다.
        ctx.registry.flush_histories(&crate::probe::SystemProbe);
        ctx.registry.kill_all();
        let status = Status { pid: std::process::id(), n_terminals: 0 };
        self.outbox.send(Frame::json(kind::STATUS, &status));
        std::thread::sleep(Duration::from_millis(200));
        exit_daemon(ctx)
    }

    /// 연결이 끊겼을 때 정리한다.
    fn close(
        &mut self,
        ctx: &Context,
    ) {
        self.release(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_protocol::peer::current_user;
    use std::io;

    struct Credentials(io::Result<UserId>);

    impl PeerCredentials for Credentials {
        fn peer_user(&self) -> io::Result<UserId> {
            match &self.0 {
                Ok(user) => Ok(user.clone()),
                Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
            }
        }
    }

    #[test]
    fn test_c_t1_peer_user_mismatch_and_unverifiable_peer_rejected() {
        let _home = crate::test_home::TempHome::new();
        let own = current_user().unwrap();
        assert!(admit(Credentials(Ok(own.clone())), &own).is_some());
        assert!(admit(Credentials(Ok(UserId::from_raw("someone-else"))), &own).is_none(), "other user must be refused");
        // 사용자를 확인하지 못한 접속자도 다른 사용자와 같이 거부된다.
        assert!(admit(Credentials(Err(io::Error::other("no credentials"))), &own).is_none());
    }
}
