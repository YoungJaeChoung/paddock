//! 데몬 제어 연결: 데몬에 인사하고, 연결이 끊길 때까지 붙들고 있는다.
//!
//! 이 연결이 곧 게이트웨이의 수명이다. 데몬이 끝나면(앱의 `Quit and Stop All Terminals` 포함) 연결이 끊겨 게이트웨이도 끝난다.
use std::fmt;
use std::io;

use interprocess::local_socket::Stream;
use paddock_protocol::daemon_info::{DaemonInfo, read_info};
use paddock_protocol::endpoint::connect_verified;
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Hello, HelloOk, PROTOCOL_VERSION, kind};
use paddock_protocol::peer::PeerError;

/// 데몬에 붙지 못한 이유.
#[derive(Debug)]
pub enum LinkError {
    /// `daemon.json`이 없거나 그 데몬이 살아 있지 않다.
    NoDaemon,
    Peer(PeerError),
    /// 인사가 거부됐거나 답이 이상하다.
    Handshake(String),
    Io(io::Error),
}

impl fmt::Display for LinkError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            LinkError::NoDaemon => f.write_str("the terminal daemon is not running"),
            LinkError::Peer(error) => write!(f, "{error}"),
            LinkError::Handshake(message) => write!(f, "daemon handshake failed: {message}"),
            LinkError::Io(error) => write!(f, "daemon connection failed: {error}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// 인사를 마친 데몬 연결.
pub struct DaemonLink {
    pub stream: Stream,
    pub info: DaemonInfo,
    /// 데몬의 프로토콜 버전이 이 게이트웨이와 같은가. 다르면 인사만 받아들인 연결이라 터미널을 잇지 못한다.
    pub compatible: bool,
}

/// `daemon.json`의 데몬에 붙어 인사한다. 같은 사용자의 데몬일 때만 토큰을 보낸다.
pub fn connect() -> Result<DaemonLink, LinkError> {
    let info = read_info().filter(DaemonInfo::is_running).ok_or(LinkError::NoDaemon)?;
    let mut stream = connect_verified(&info.endpoint).map_err(LinkError::Peer)?;
    Frame::json(kind::HELLO, &Hello { token: info.token.clone(), protocol_version: PROTOCOL_VERSION })
        .write_to(&mut stream)
        .map_err(LinkError::Io)?;
    let reply = Frame::read_from(&mut stream).map_err(LinkError::Io)?;
    match reply.kind {
        kind::HELLO_OK => {
            let ok: HelloOk = reply.parse_json().map_err(LinkError::Io)?;
            Ok(DaemonLink { stream, info, compatible: ok.protocol_version == PROTOCOL_VERSION })
        }
        kind::ERR => Err(LinkError::Handshake(reply.as_text())),
        other => Err(LinkError::Handshake(format!("unexpected reply kind {other}"))),
    }
}

/// 데몬이 연결을 끊을 때까지 읽는다. 이 연결로는 요청하지 않으므로 오는 프레임은 버린다.
pub fn wait_until_closed(mut stream: Stream) {
    while Frame::read_from(&mut stream).is_ok() {}
}
