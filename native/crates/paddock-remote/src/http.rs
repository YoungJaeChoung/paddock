//! 브라우저가 접속하는 HTTP 서버: 127.0.0.1 듣기, 요청 머리 읽기, 정적 화면, `POST /pair`, WebSocket 승격.
//!
//! 모든 요청은 `gate::check`를 지난 뒤에만 처리한다. 비동기 런타임 없이 접속마다 스레드를 하나 쓰고, 동시 접속 수·머리 크기·읽기 시간에 상한이 있다.
//! TLS는 없다 — 밖에서 오는 연결은 `tailscale serve`가 받아 이 루프백 포트로 넘긴다.
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::devices::DeviceCookie;
use crate::gate::{self, Admission, RequestHead, Route};
use crate::gateway::Gateway;
use crate::log::{self, Event};
use crate::pairing::Submission;
use crate::websocket;

/// 정적 화면. 실행 파일에 묶어 외부 자원이 없다.
const INDEX_HTML: &[u8] = include_bytes!("../web/index.html");

/// 서버가 지키는 상한.
pub struct Limits;

impl Limits {
    /// 요청 머리(요청 줄과 헤더)의 최대 바이트.
    pub const HEAD_BYTES: usize = 8 * 1024;
    /// 헤더 최대 개수.
    pub const HEADERS: usize = 48;
    /// 머리를 다 읽기까지 허용하는 시간.
    pub const HEAD_TIME: Duration = Duration::from_secs(5);
    /// 접속 뒤 첫 바이트가 오기까지 허용하는 시간. 아무것도 안 보내고 자리만 쥐는 접속을 빨리 놓게 한다.
    pub const FIRST_BYTE_TIME: Duration = Duration::from_secs(2);
    /// `POST /pair` 본문의 최대 바이트와 읽기 시간.
    pub const BODY_BYTES: usize = 2 * 1024;
    pub const BODY_TIME: Duration = Duration::from_secs(5);
    /// 응답을 쓰는 데 허용하는 시간.
    pub const WRITE_TIME: Duration = Duration::from_secs(10);
    /// 인증 전 단계(요청 머리를 읽고 게이트를 지나는 중)의 동시 접속 수. 넘으면 새 접속을 바로 끊는다.
    /// WebSocket으로 승격한 접속은 이 수에서 빠지고 `Connections`의 한도(기기당·전체)를 따로 받는다 —
    /// 열어 둔 WebSocket이나 말없는 접속이 정상 요청의 자리를 막지 못하게 한다.
    pub const PRE_AUTH_CONNECTIONS: usize = 64;
}

/// 127.0.0.1에서 듣는 리스너를 연다. `port`가 0이면 운영체제가 빈 포트를 고른다.
///
/// 게이트웨이가 어디서 듣는지는 이 함수 하나가 정한다 — 0.0.0.0이나 테일넷 주소가 아니라 루프백만 쓴다.
pub fn bind_loopback(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

/// 머리를 읽다 실패한 이유.
#[derive(Debug)]
pub enum HeadError {
    /// 읽기 시간 안에 머리가 끝나지 않았다.
    Timeout,
    /// 머리가 `HEAD_BYTES`나 `HEADERS`를 넘었다.
    TooLarge,
    /// 요청 형식이 틀렸다.
    Malformed,
    /// 머리를 다 받기 전에 상대가 끊었다.
    Closed,
    Io(io::Error),
}

/// 읽은 요청: 게이트가 보는 머리와 처리에 필요한 나머지.
#[derive(Debug)]
pub struct ParsedRequest {
    pub head: RequestHead,
    pub content_length: Option<usize>,
    pub has_transfer_encoding: bool,
    pub websocket_key: Option<String>,
    pub websocket_version: Option<String>,
    /// 머리를 읽다가 함께 받은 본문 앞부분.
    pub body_prefix: Vec<u8>,
}

/// 접속을 받아 접속마다 스레드에서 처리한다. 돌아오지 않는다.
pub fn serve(
    listener: TcpListener,
    gateway: Arc<Gateway>,
) {
    let n_pre_auth = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        if n_pre_auth.fetch_add(1, Ordering::SeqCst) >= Limits::PRE_AUTH_CONNECTIONS {
            n_pre_auth.fetch_sub(1, Ordering::SeqCst);
        } else {
            let gateway = gateway.clone();
            let slot = PreAuthSlot { n_pre_auth: n_pre_auth.clone(), is_held: true };
            std::thread::spawn(move || handle(stream, &gateway, slot));
        }
    }
}

/// 인증 전 접속 한 자리. 버리거나 `release`하면 자리가 빈다.
struct PreAuthSlot {
    n_pre_auth: Arc<AtomicUsize>,
    is_held: bool,
}

impl PreAuthSlot {
    /// 자리를 비운다(WebSocket으로 승격한 접속이 쓴다). 두 번 불러도 한 번만 비운다.
    fn release(&mut self) {
        if self.is_held {
            self.is_held = false;
            self.n_pre_auth.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for PreAuthSlot {
    fn drop(&mut self) {
        self.release();
    }
}

/// 접속 하나를 처음부터 끝까지 처리한다.
fn handle(
    mut stream: TcpStream,
    gateway: &Gateway,
    slot: PreAuthSlot,
) {
    let _ = stream.set_write_timeout(Some(Limits::WRITE_TIME));
    let _ = stream.set_nodelay(true);
    match read_request(&mut stream, Instant::now() + Limits::HEAD_TIME) {
        Ok(request) => dispatch(stream, request, gateway, slot),
        Err(HeadError::Timeout) => respond_empty(&mut stream, 408),
        Err(HeadError::TooLarge) => respond_empty(&mut stream, 431),
        Err(HeadError::Malformed) => respond_empty(&mut stream, 400),
        Err(HeadError::Closed | HeadError::Io(_)) => {}
    }
}

/// 요청을 게이트에 통과시킨 뒤 길에 맞게 처리한다.
fn dispatch(
    mut stream: TcpStream,
    request: ParsedRequest,
    gateway: &Gateway,
    slot: PreAuthSlot,
) {
    let verdict = gateway
        .devices
        .with_records(|records| gate::check(&request.head, &gateway.config, records, Gateway::now_secs()));
    match verdict {
        Err(denial) => {
            gateway
                .log_limited(&Event::Denied { route: gate::route_of(&request.head).name(), reason: &denial.reason() });
            respond_empty(&mut stream, denial.status());
        }
        Ok(admission) => match admission.route {
            Route::Static => respond(&mut stream, 200, Some("text/html; charset=utf-8"), INDEX_HTML, None),
            Route::Pair => pair(&mut stream, &request, gateway),
            Route::WebSocket => upgrade(stream, &request, &admission, gateway, slot),
            Route::Unknown => respond_empty(&mut stream, 404),
        },
    }
}

/// `POST /pair`의 본문.
#[derive(Deserialize)]
struct PairRequest {
    code: String,
    #[serde(default)]
    name: String,
}

/// 기기 승인 코드를 받아 맞으면 기기 토큰 쿠키를 심는다.
///
/// 틀린 코드·닫힌 창·만료는 같은 응답이라 밖에서는 창이 열려 있는지 알 수 없다.
fn pair(
    stream: &mut TcpStream,
    request: &ParsedRequest,
    gateway: &Gateway,
) {
    match read_body(stream, request) {
        Err(status) => respond_empty(stream, status),
        Ok(body) => match serde_json::from_slice::<PairRequest>(&body) {
            Err(_) => respond_empty(stream, 400),
            Ok(pair_request) => match gateway.pairing.submit(&pair_request.code, Instant::now()) {
                Submission::Accepted => match gateway.devices.add(&pair_request.name, Gateway::now_secs()) {
                    Ok((info, token)) => {
                        log::write(&Event::PairingSucceeded { device: &info.name });
                        let cookie = DeviceCookie::set_header(&token);
                        respond(stream, 200, Some("application/json"), br#"{"ok":true}"#, Some(&cookie));
                    }
                    Err(error) => {
                        log::write(&Event::DeviceSaveFailed { error_kind: &format!("{:?}", error.kind()) });
                        respond_empty(stream, 500);
                    }
                },
                Submission::Wrong { n_failures, closed } => {
                    gateway.log_limited(&Event::PairingFailed { n_failures, closed });
                    respond(stream, 403, Some("application/json"), br#"{"error":"pairing failed"}"#, None);
                }
                Submission::NotOpen => {
                    gateway.log_limited(&Event::PairingRejectedNotOpen);
                    respond(stream, 403, Some("application/json"), br#"{"error":"pairing failed"}"#, None);
                }
            },
        },
    }
}

/// `Content-Length`만큼 본문을 읽는다. 실패하면 응답할 상태 코드를 돌려준다.
fn read_body(
    stream: &mut TcpStream,
    request: &ParsedRequest,
) -> Result<Vec<u8>, u16> {
    match request.content_length {
        _ if request.has_transfer_encoding => Err(411),
        None => Err(411),
        Some(length) if length > Limits::BODY_BYTES => Err(413),
        Some(length) => {
            let deadline = Instant::now() + Limits::BODY_TIME;
            let mut body = request.body_prefix.clone();
            body.truncate(length);
            let mut chunk = [0u8; 512];
            let mut result = Ok(());
            while body.len() < length && result.is_ok() {
                let step = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or(408)
                    .and_then(|remaining| stream.set_read_timeout(Some(remaining)).map_err(|_| 400))
                    .and_then(|()| match stream.read(&mut chunk) {
                        Ok(0) => Err(400),
                        Ok(n_read) => Ok(n_read),
                        Err(error) if is_timeout(&error) => Err(408),
                        Err(_) => Err(400),
                    });
                match step {
                    Ok(n_read) => body.extend_from_slice(&chunk[..n_read.min(length - body.len())]),
                    Err(status) => result = Err(status),
                }
            }
            result.map(|()| body)
        }
    }
}

/// WebSocket으로 승격한다. 열린 연결 수 상한을 넘으면 429, 승격 헤더가 틀리면 400이다.
///
/// 연결을 목록에 올린 직후 기기가 아직 유효한지 다시 본다. 게이트를 지난 뒤 폐기가 끝났다면 목록에는 아직 없었던 이 연결이
/// 폐기의 연결 끊기를 피해 갈 수 있기 때문이다(폐기는 기록을 바꾼 뒤 연결을 끊으므로, 올린 뒤의 재확인과 합치면 틈이 없다).
fn upgrade(
    mut stream: TcpStream,
    request: &ParsedRequest,
    admission: &Admission,
    gateway: &Gateway,
    mut slot: PreAuthSlot,
) {
    let key = request.websocket_key.as_deref().filter(|_| request.websocket_version.as_deref() == Some("13"));
    match (key, &admission.device) {
        (Some(key), Some((device_id, device_name))) => match gateway.connections.register(device_id, &stream) {
            None => respond_empty(&mut stream, 429),
            Some(_guard) if !gateway.devices.is_active(device_id, Gateway::now_secs()) => {
                respond_empty(&mut stream, 401)
            }
            Some(_guard) => {
                let _ = gateway.devices.touch(device_id, Gateway::now_secs());
                let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
                let response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
                );
                if stream.write_all(response.as_bytes()).is_ok() {
                    slot.release();
                    let login = request.head.login.as_deref().unwrap_or("");
                    log::write(&Event::DeviceConnected { device: device_name, login });
                    websocket::run(stream, device_name);
                    log::write(&Event::DeviceDisconnected { device: device_name });
                }
            }
        },
        _ => respond_empty(&mut stream, 400),
    }
}

/// 읽기 시간 초과 오류인가.
fn is_timeout(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// 요청 머리를 `deadline`까지 읽어 해석한다.
fn read_request(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<ParsedRequest, HeadError> {
    let started = Instant::now();
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        match parse_head(&buf) {
            Ok(Some(request)) => break Ok(request),
            Err(error) => break Err(error),
            Ok(None) => {}
        }
        if buf.len() >= Limits::HEAD_BYTES {
            break Err(HeadError::TooLarge);
        }
        let effective = if buf.is_empty() { deadline.min(started + Limits::FIRST_BYTE_TIME) } else { deadline };
        let Some(remaining) = effective.checked_duration_since(Instant::now()).filter(|remaining| !remaining.is_zero())
        else {
            break Err(HeadError::Timeout);
        };
        if let Err(error) = stream.set_read_timeout(Some(remaining)) {
            break Err(HeadError::Io(error));
        }
        match stream.read(&mut chunk) {
            Ok(0) => break Err(HeadError::Closed),
            Ok(n_read) => buf.extend_from_slice(&chunk[..n_read]),
            Err(error) if is_timeout(&error) => break Err(HeadError::Timeout),
            Err(error) => break Err(HeadError::Io(error)),
        }
    }
}

/// 지금까지 받은 바이트에서 요청 머리를 해석한다. 아직 머리가 끝나지 않았으면 `Ok(None)`이다.
///
/// Examples
/// --------
/// | 받은 바이트                          | 결과                         |
/// |--------------------------------------|------------------------------|
/// | `GET / HTTP/1.1\r\nHost: a\r\n\r\n`  | `Ok(Some(..))`               |
/// | `GET / HTTP/1.1\r\nHost: a\r\n`      | `Ok(None)`(더 받아야 한다)   |
/// | 요청 줄이 깨진 바이트                | `Err(Malformed)`             |
/// | 헤더가 `HEADERS`개를 넘음            | `Err(TooLarge)`              |
pub fn parse_head(buf: &[u8]) -> Result<Option<ParsedRequest>, HeadError> {
    let mut headers = [httparse::EMPTY_HEADER; Limits::HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(buf) {
        Ok(httparse::Status::Partial) => Ok(None),
        Err(httparse::Error::TooManyHeaders) => Err(HeadError::TooLarge),
        Err(_) => Err(HeadError::Malformed),
        Ok(httparse::Status::Complete(head_len)) => {
            let mut head = RequestHead {
                method: parsed.method.unwrap_or_default().to_string(),
                path: parsed.path.unwrap_or_default().split('?').next().unwrap_or_default().to_string(),
                ..RequestHead::default()
            };
            let mut request = ParsedRequest {
                head: RequestHead::default(),
                content_length: None,
                has_transfer_encoding: false,
                websocket_key: None,
                websocket_version: None,
                body_prefix: buf[head_len..].to_vec(),
            };
            let mut has_upgrade = false;
            let mut has_connection_upgrade = false;
            let mut is_malformed = false;
            for header in parsed.headers.iter() {
                let value = String::from_utf8_lossy(header.value).trim().to_string();
                let name = header.name.to_ascii_lowercase();
                match name.as_str() {
                    "host" => set_once(&mut head.host, &mut head.duplicate_header, "host", value),
                    "tailscale-user-login" => set_once(&mut head.login, &mut head.duplicate_header, "login", value),
                    "origin" => set_once(&mut head.origin, &mut head.duplicate_header, "origin", value),
                    "cookie" => set_once(&mut head.cookie, &mut head.duplicate_header, "cookie", value),
                    "upgrade" => has_upgrade = value.eq_ignore_ascii_case("websocket"),
                    "connection" => {
                        has_connection_upgrade |=
                            value.split(',').any(|token| token.trim().eq_ignore_ascii_case("upgrade"));
                    }
                    "content-length" => match (value.parse::<usize>(), request.content_length) {
                        (Ok(length), None) => request.content_length = Some(length),
                        _ => is_malformed = true,
                    },
                    "transfer-encoding" => request.has_transfer_encoding = true,
                    "sec-websocket-key" => request.websocket_key = Some(value),
                    "sec-websocket-version" => request.websocket_version = Some(value),
                    _ => {}
                }
            }
            head.is_websocket_upgrade = has_upgrade && has_connection_upgrade;
            request.head = head;
            if is_malformed { Err(HeadError::Malformed) } else { Ok(Some(request)) }
        }
    }
}

/// 한 번만 와야 하는 헤더를 채운다. 이미 채워져 있으면 중복으로 표시한다.
fn set_once(
    slot: &mut Option<String>,
    duplicate: &mut Option<&'static str>,
    name: &'static str,
    value: String,
) {
    if slot.is_some() {
        *duplicate = Some(name);
    } else {
        *slot = Some(value);
    }
}

/// 모든 응답에 붙는 보안 헤더. 외부 자원 0·프레임 삽입 금지·형식 추측 금지·참조 주소 미전송.
const SECURITY_HEADERS: &str = "Content-Security-Policy: default-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\n";

/// 본문 없는 응답을 보낸다.
fn respond_empty(
    stream: &mut TcpStream,
    status: u16,
) {
    respond(stream, status, None, b"", None);
}

/// 응답을 쓰고 연결을 닫는다. 쓰기 오류는 상대가 끊은 것이라 버린다.
fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: Option<&str>,
    body: &[u8],
    set_cookie: Option<&str>,
) {
    let mut head = format!("HTTP/1.1 {status} {}\r\n{SECURITY_HEADERS}Connection: close\r\n", reason_phrase(status));
    if let Some(content_type) = content_type {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    if let Some(cookie) = set_cookie {
        head.push_str(&format!("Set-Cookie: {cookie}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body)).and_then(|()| stream.flush());
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        _ => "Internal Server Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_head_reads_the_headers_the_gate_needs() {
        let raw = b"GET /ws?x=1 HTTP/1.1\r\nHost: a:1\r\nTailscale-User-Login: me@x\r\nOrigin: https://a:1\r\nCookie: k=v\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: abc\r\nSec-WebSocket-Version: 13\r\n\r\n";
        let request = parse_head(raw).unwrap().unwrap();
        assert_eq!(request.head.method, "GET");
        assert_eq!(request.head.path, "/ws");
        assert_eq!(request.head.host.as_deref(), Some("a:1"));
        assert_eq!(request.head.login.as_deref(), Some("me@x"));
        assert_eq!(request.head.origin.as_deref(), Some("https://a:1"));
        assert_eq!(request.head.cookie.as_deref(), Some("k=v"));
        assert!(request.head.is_websocket_upgrade);
        assert_eq!(request.websocket_key.as_deref(), Some("abc"));
        assert_eq!(request.head.duplicate_header, None);
    }

    #[test]
    fn test_parse_head_needs_both_upgrade_headers() {
        let only_upgrade = b"GET /ws HTTP/1.1\r\nHost: a\r\nUpgrade: websocket\r\n\r\n";
        assert!(!parse_head(only_upgrade).unwrap().unwrap().head.is_websocket_upgrade);
        let only_connection = b"GET /ws HTTP/1.1\r\nHost: a\r\nConnection: Upgrade\r\n\r\n";
        assert!(!parse_head(only_connection).unwrap().unwrap().head.is_websocket_upgrade);
    }

    #[test]
    fn test_parse_head_marks_duplicate_security_headers() {
        let raw = b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n";
        assert_eq!(parse_head(raw).unwrap().unwrap().head.duplicate_header, Some("host"));
        let raw = b"GET / HTTP/1.1\r\nTailscale-User-Login: a\r\nTailscale-User-Login: a\r\n\r\n";
        assert_eq!(parse_head(raw).unwrap().unwrap().head.duplicate_header, Some("login"));
    }

    #[test]
    fn test_parse_head_partial_malformed_and_too_many_headers() {
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: a\r\n").unwrap().is_none());
        assert!(matches!(parse_head(b"\x00\x01 garbage\r\n\r\n"), Err(HeadError::Malformed)));
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for index in 0..=Limits::HEADERS {
            raw.push_str(&format!("X-{index}: 1\r\n"));
        }
        raw.push_str("\r\n");
        assert!(matches!(parse_head(raw.as_bytes()), Err(HeadError::TooLarge)));
    }

    #[test]
    fn test_parse_head_content_length_rules_and_body_prefix() {
        let raw = b"POST /pair HTTP/1.1\r\nContent-Length: 5\r\n\r\nab";
        let request = parse_head(raw).unwrap().unwrap();
        assert_eq!(request.content_length, Some(5));
        assert_eq!(request.body_prefix, b"ab");
        let twice = b"POST /pair HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\n";
        assert!(matches!(parse_head(twice), Err(HeadError::Malformed)));
        let negative = b"POST /pair HTTP/1.1\r\nContent-Length: -1\r\n\r\n";
        assert!(matches!(parse_head(negative), Err(HeadError::Malformed)));
        let chunked = b"POST /pair HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(parse_head(chunked).unwrap().unwrap().has_transfer_encoding);
    }

    #[test]
    fn test_bind_loopback_listens_on_127_0_0_1_only() {
        let listener = bind_loopback(0).unwrap();
        let address = listener.local_addr().unwrap();
        assert_eq!(address.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(address.port(), 0);
    }
}
