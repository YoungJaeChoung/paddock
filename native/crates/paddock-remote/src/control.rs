//! 앱이 접속하는 제어 소켓: 상태 읽기·기기 승인 창 열기·기기 목록·폐기·끝내기.
//!
//! 소켓은 소유자 전용이고, 접속자가 이 게이트웨이와 같은 사용자일 때만 요청을 받는다.
//! 페어링 코드는 이 응답으로만 나가므로 디스크에 닿지 않는다.
use std::io::{self, Write};
use std::sync::Arc;

use interprocess::local_socket::Listener;
use interprocess::local_socket::traits::ListenerExt as _;
use paddock_protocol::frame::Frame;
use paddock_protocol::peer::{PeerCredentials, PeerRole, UserId, current_user, require_same_user};
use paddock_protocol::remote_control::{DevicesReply, Revoke, kind};

use crate::gateway::{Gateway, StopReason};
use crate::log::{self, Event};

/// 접속자가 `own`과 같은 사용자일 때만 연결을 돌려준다. 다르거나 확인하지 못하면 한 바이트도 보내지 않고 연결을 버린다.
pub fn admit<S: PeerCredentials>(
    stream: S,
    own: &UserId,
) -> Option<S> {
    require_same_user(stream, PeerRole::Client, own).ok()
}

/// 제어 접속을 받아 접속마다 스레드에서 처리한다. 돌아오지 않는다.
pub fn serve(
    listener: Listener,
    gateway: Arc<Gateway>,
) {
    let Ok(own) = current_user() else {
        return;
    };
    for stream in listener.incoming().flatten() {
        let gateway = gateway.clone();
        let own = own.clone();
        std::thread::spawn(move || match admit(stream, &own) {
            Some(stream) => handle(stream, &gateway),
            None => log::write(&Event::ControlRefused),
        });
    }
}

/// 연결이 끊길 때까지 요청마다 답을 하나씩 보낸다.
fn handle<S: io::Read + Write>(
    mut stream: S,
    gateway: &Gateway,
) {
    while let Ok(request) = Frame::read_from(&mut stream) {
        let reply = reply_to(&request, gateway);
        let is_stop = request.kind == kind::STOP && reply.kind == kind::OK;
        if reply.write_to(&mut stream).is_err() {
            break;
        }
        if is_stop {
            gateway.request_stop(StopReason::ControlStop);
        }
    }
}

/// 요청 하나의 답을 만든다. 모르는 종류와 모양이 틀린 내용은 `ERR`다.
fn reply_to(
    request: &Frame,
    gateway: &Gateway,
) -> Frame {
    match request.kind {
        kind::STATUS => Frame::json(kind::STATUS_REPLY, &gateway.status()),
        kind::OPEN_PAIRING => Frame::json(kind::PAIRING_OPENED, &gateway.open_pairing()),
        kind::DEVICES => Frame::json(kind::DEVICES_REPLY, &DevicesReply { devices: gateway.devices_info() }),
        kind::REVOKE => match request.parse_json::<Revoke>() {
            Err(_) => Frame::text(kind::ERR, "Bad revoke request"),
            Ok(revoke) => match gateway.revoke(&revoke.id) {
                Ok(true) => Frame::new(kind::OK, Vec::new()),
                Ok(false) => Frame::text(kind::ERR, "No such device"),
                Err(error) => Frame::text(kind::ERR, &format!("Could not save devices: {error}")),
            },
        },
        kind::STOP => Frame::new(kind::OK, Vec::new()),
        _ => Frame::text(kind::ERR, "Unknown request"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_other_user_and_unverifiable_peer_are_refused() {
        let own = current_user().unwrap();
        assert!(admit(Credentials(Ok(own.clone())), &own).is_some());
        assert!(admit(Credentials(Ok(UserId::from_raw("someone-else"))), &own).is_none(), "other user must be refused");
        assert!(admit(Credentials(Err(io::Error::other("no credentials"))), &own).is_none());
    }
}
