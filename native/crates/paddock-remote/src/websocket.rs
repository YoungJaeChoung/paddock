//! 인증을 마친 기기의 WebSocket. 이 단계에서는 인사(hello)를 보내고 연결을 유지할 뿐 터미널은 잇지 않는다.
use std::net::TcpStream;
use std::time::{Duration, Instant};

use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message, WebSocket};

/// 연결 유지 규칙.
pub struct KeepAlive;

impl KeepAlive {
    /// 이 시간 동안 아무것도 안 받으면 핑을 보낸다.
    pub const PING_AFTER: Duration = Duration::from_secs(30);
    /// 이 시간 동안 아무것도 못 받으면 연결을 끊는다.
    pub const DROP_AFTER: Duration = Duration::from_secs(90);
    /// 받는 메시지와 프레임의 최대 바이트.
    pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
}

/// 승격을 마친 소켓으로 WebSocket을 돌린다. 상대가 닫거나 끊거나 조용해지면 돌아온다.
///
/// 인사 한 통을 보낸 뒤 받은 메시지는 모두 버린다(터미널 잇기는 다음 단계의 몫이다).
pub fn run(
    stream: TcpStream,
    device_name: &str,
) {
    let _ = stream.set_read_timeout(Some(KeepAlive::PING_AFTER));
    let config = WebSocketConfig::default()
        .max_message_size(Some(KeepAlive::MAX_MESSAGE_BYTES))
        .max_frame_size(Some(KeepAlive::MAX_MESSAGE_BYTES));
    let mut socket = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    let hello = serde_json::json!({ "type": "hello", "device": device_name, "version": 1 }).to_string();
    let mut is_open = socket.send(Message::text(hello)).is_ok();
    let mut last_heard = Instant::now();
    while is_open {
        match socket.read() {
            Ok(Message::Close(_)) => is_open = false,
            Ok(_) => last_heard = Instant::now(),
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) =>
            {
                is_open = last_heard.elapsed() < KeepAlive::DROP_AFTER
                    && socket.send(Message::Ping(Vec::new().into())).is_ok();
            }
            Err(_) => is_open = false,
        }
    }
    let _ = socket.close(None);
    let _ = socket.flush();
}
