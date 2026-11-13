//! 게이트웨이 실행 파일을 임시 홈에서 실제로 띄워 HTTP·WebSocket·제어 소켓 전 구간을 확인한다.
//!
//! 데몬은 가짜다: `daemon.json`을 쓰고 인사에만 답하는 리스너를 시험 안에서 돌린다. 시험이 띄운 게이트웨이 프로세스만 끝낸다.
// 소켓 파일 경로(`run/daemon.sock`·`run/remote.sock`)를 직접 짚으므로 Unix에서만 돈다. Windows 엔드포인트 규칙은 `paddock-protocol`의 시험이 맡는다.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt as _;
use paddock_protocol::daemon_info::DaemonInfo;
use paddock_protocol::endpoint::{connect_verified, create_listener};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{HelloOk, PROTOCOL_VERSION, kind as daemon_kind};
use paddock_protocol::remote_control::{DevicesReply, GatewayStatus, PairingOpened, Revoke, kind};
use paddock_remote::devices::{DeviceCookie, DeviceRecord, hash_token};
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

const HOST: &str = "mac.tail1234.ts.net:8443";
const LOGIN: &str = "me@example.com";

/// 인사에만 답하는 가짜 데몬. 버리거나 `drop_connection`을 부르면 게이트웨이의 연결을 끊는다.
struct FakeDaemon {
    close: Sender<()>,
}

impl FakeDaemon {
    fn start(home: &Path) -> Self {
        let native = home.join(".paddock").join("native");
        let endpoint = native.join("run").join("daemon.sock").to_string_lossy().into_owned();
        std::fs::create_dir_all(native.join("run")).unwrap();
        let listener = create_listener(&endpoint).unwrap();
        let info = DaemonInfo {
            pid: std::process::id(),
            endpoint,
            token: "daemon-token".into(),
            fingerprint: "fp".into(),
            started: "now".into(),
            breakaway_denied: false,
        };
        std::fs::write(native.join("daemon.json"), serde_json::to_string(&info).unwrap()).unwrap();
        let (close, closed) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            if let Some(mut stream) = listener.incoming().flatten().next() {
                let hello = Frame::read_from(&mut stream).unwrap();
                assert_eq!(hello.kind, daemon_kind::HELLO);
                let ok =
                    HelloOk { pid: std::process::id(), fingerprint: "fp".into(), protocol_version: PROTOCOL_VERSION };
                Frame::json(daemon_kind::HELLO_OK, &ok).write_to(&mut stream).unwrap();
                // 끊으라는 신호가 올 때까지 연결을 쥔다(신호 보내는 쪽이 사라져도 끊는다).
                let _ = closed.recv();
                drop(stream);
            }
        });
        Self { close }
    }

    fn drop_connection(&self) {
        let _ = self.close.send(());
    }
}

/// 띄운 게이트웨이 프로세스. 버릴 때 그 프로세스만 끝낸다.
struct Gateway {
    child: Child,
    port: u16,
    home: PathBuf,
}

impl Gateway {
    /// `devices_json`이 있으면 시작 전에 기기 목록 파일로 쓴다.
    fn start(
        home: &Path,
        devices_json: Option<String>,
    ) -> Self {
        if let Some(text) = devices_json {
            let dir = home.join(".paddock").join("native").join("remote");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("devices.json"), text).unwrap();
        }
        let mut child = Self::spawn(home, &[]);
        let port = Self::read_port(&mut child);
        Self { child, port, home: home.to_path_buf() }
    }

    fn spawn(
        home: &Path,
        extra_args: &[&str],
    ) -> Child {
        Command::new(env!("CARGO_BIN_EXE_paddock-remote"))
            .args(["--host", HOST, "--login", LOGIN])
            .args(extra_args)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env_remove("XDG_RUNTIME_DIR")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn paddock-remote")
    }

    fn read_port(child: &mut Child) -> u16 {
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        line.trim()
            .rsplit(':')
            .next()
            .and_then(|port| port.parse().ok())
            .unwrap_or_else(|| panic!("no port line: {line:?}"))
    }

    fn control_endpoint(&self) -> String {
        self.home.join(".paddock").join("native").join("run").join("remote.sock").to_string_lossy().into_owned()
    }

    fn control(
        &self,
        kind: u8,
        payload: Vec<u8>,
    ) -> Frame {
        let mut stream = connect_verified(&self.control_endpoint()).expect("control connect");
        Frame::new(kind, payload).write_to(&mut stream).unwrap();
        Frame::read_from(&mut stream).unwrap()
    }

    fn status(&self) -> GatewayStatus {
        self.control(kind::STATUS, Vec::new()).parse_json().unwrap()
    }

    fn open_pairing(&self) -> PairingOpened {
        self.control(kind::OPEN_PAIRING, Vec::new()).parse_json().unwrap()
    }

    fn devices(&self) -> DevicesReply {
        self.control(kind::DEVICES, Vec::new()).parse_json().unwrap()
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.home.join(".paddock/native/logs/remote.log")).unwrap_or_default()
    }

    /// 프로세스가 끝나기를 기다려 종료 코드를 돌려준다. 시간 안에 안 끝나면 `None`이다.
    fn wait_exit(
        &mut self,
        within: Duration,
    ) -> Option<i32> {
        let end = Instant::now() + within;
        let mut code = None;
        while code.is_none() && Instant::now() < end {
            match self.child.try_wait().unwrap() {
                Some(status) => code = Some(status.code().unwrap_or(-1)),
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        code
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 요청 하나를 보내고 응답(상태 코드, 머리 전체, 본문)을 받는다.
struct Response {
    status: u16,
    head: String,
    body: String,
}

fn raw_request(
    port: u16,
    request: &str,
) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|code| code.parse().ok()).unwrap_or(0);
    Response { status, head: head.to_string(), body: body.to_string() }
}

/// 헤더 목록으로 요청 텍스트를 만든다. `None`인 헤더는 빠진다.
fn request_text(
    method: &str,
    path: &str,
    headers: &[(&str, Option<String>)],
    body: &str,
) -> String {
    let mut text = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in headers {
        if let Some(value) = value {
            text.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    if !body.is_empty() || method == "POST" {
        text.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    text.push_str("Connection: close\r\n\r\n");
    text.push_str(body);
    text
}

/// 모든 검사를 통과하는 머리에서 `overrides`로 헤더를 바꾸거나(`Some`) 뺀다(`None`).
fn headers_with(overrides: &[(&'static str, Option<String>)]) -> Vec<(&'static str, Option<String>)> {
    let mut headers: Vec<(&'static str, Option<String>)> = vec![
        ("Host", Some(HOST.to_string())),
        ("Tailscale-User-Login", Some(LOGIN.to_string())),
        ("Origin", Some(format!("https://{HOST}"))),
    ];
    for (name, value) in overrides {
        headers.retain(|(existing, _)| existing != name);
        headers.push((name, value.clone()));
    }
    headers
}

fn get(
    port: u16,
    path: &str,
    overrides: &[(&'static str, Option<String>)],
) -> Response {
    raw_request(port, &request_text("GET", path, &headers_with(overrides), ""))
}

fn post_pair(
    port: u16,
    code: &str,
    overrides: &[(&'static str, Option<String>)],
) -> Response {
    let body = format!(r#"{{"code":"{code}","name":"Test phone"}}"#);
    raw_request(port, &request_text("POST", "/pair", &headers_with(overrides), &body))
}

/// 페어링을 마치고 기기 토큰을 돌려준다.
fn pair_device(gateway: &Gateway) -> String {
    let opened = gateway.open_pairing();
    let response = post_pair(gateway.port, &opened.code, &[]);
    assert_eq!(response.status, 200, "{}", response.head);
    cookie_token(&response.head)
}

/// `Set-Cookie` 머리에서 기기 토큰을 꺼낸다.
fn cookie_token(head: &str) -> String {
    let line = head.lines().find(|line| line.starts_with("Set-Cookie: ")).expect("Set-Cookie header");
    line.trim_start_matches("Set-Cookie: ")
        .trim_start_matches(&format!("{}=", DeviceCookie::NAME))
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

/// WebSocket 승격 요청을 보내고 (응답 상태, 이어받은 소켓)을 돌려준다.
fn open_websocket(
    port: u16,
    cookie: Option<String>,
) -> (u16, TcpStream) {
    let mut headers = headers_with(&[]);
    headers.push(("Cookie", cookie));
    headers.push(("Upgrade", Some("websocket".into())));
    headers.push(("Connection", Some("Upgrade".into())));
    headers.push(("Sec-WebSocket-Key", Some("dGhlIHNhbXBsZSBub25jZQ==".into())));
    headers.push(("Sec-WebSocket-Version", Some("13".into())));
    let mut text = String::from("GET /ws HTTP/1.1\r\n");
    for (name, value) in &headers {
        if let Some(value) = value {
            text.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    text.push_str("\r\n");
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    // 머리 끝(빈 줄)까지 한 바이트씩 읽는다 — 뒤따르는 WebSocket 프레임을 먹지 않으려는 것이다.
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let status = head.split(' ').nth(1).and_then(|code| code.parse().ok()).unwrap_or(0);
    (status, stream)
}

fn cookie_header(token: &str) -> Option<String> {
    Some(format!("{}={token}", DeviceCookie::NAME))
}

fn new_home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn record_json(
    token: &str,
    last_seen: u64,
    revoked: Option<u64>,
) -> DeviceRecord {
    DeviceRecord {
        id: format!("id-{last_seen}"),
        name: "Old phone".into(),
        token_hash: hash_token(token),
        created: last_seen,
        last_seen,
        revoked,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn test_gate_table_denies_each_wrong_request_and_records_the_reason() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let now = now_secs();
    let thirty_one_days = 31 * 24 * 3600;
    let devices = serde_json::json!({
        "version": 1,
        "devices": [
            record_json("revoked-token", now - 100, Some(now - 50)),
            record_json("expired-token", now - thirty_one_days, None),
        ]
    });
    let gateway = Gateway::start(home.path(), Some(devices.to_string()));
    let port = gateway.port;
    let good_token = pair_device(&gateway);

    // (설명, 응답 상태, 기록에 남을 이유, 요청)
    let ws_cookie = cookie_header;
    let table: Vec<(&str, u16, &str, Response)> = vec![
        ("host wrong", 403, "host-mismatch", get(port, "/", &[("Host", Some("evil.example:8443".into()))])),
        ("host without port", 403, "host-mismatch", get(port, "/", &[("Host", Some("mac.tail1234.ts.net".into()))])),
        ("login missing", 403, "login-missing", get(port, "/", &[("Tailscale-User-Login", None)])),
        (
            "login other",
            403,
            "login-mismatch",
            get(port, "/", &[("Tailscale-User-Login", Some("other@example.com".into()))]),
        ),
        (
            "pair origin wrong",
            403,
            "origin-mismatch",
            post_pair(port, "x", &[("Origin", Some("https://evil.example".into()))]),
        ),
        ("pair origin missing", 403, "origin-missing", post_pair(port, "x", &[("Origin", None)])),
    ];
    for (name, status, reason, response) in &table {
        assert_eq!(response.status, *status, "{name}: {}", response.head);
        assert!(response.body.is_empty(), "{name}: denied requests carry no body");
        assert!(gateway.log_text().contains(&format!(": {reason}")), "{name}: reason {reason} must be logged");
    }

    // WebSocket 쪽 거부(쿠키·폐기·만료·Origin).
    let (status, _) = open_websocket(port, None);
    assert_eq!(status, 401, "cookie missing");
    let (status, _) = open_websocket(port, ws_cookie("never-issued"));
    assert_eq!(status, 401, "unknown token");
    let (status, _) = open_websocket(port, ws_cookie("revoked-token"));
    assert_eq!(status, 401, "revoked token");
    let (status, _) = open_websocket(port, ws_cookie("expired-token"));
    assert_eq!(status, 401, "expired token");
    let log = gateway.log_text();
    for reason in ["token-missing", "token-unknown", "token-revoked", "token-expired"] {
        assert!(log.contains(&format!("denied websocket: {reason}")), "{reason} must be logged:\n{log}");
    }

    // 맞는 토큰은 같은 경로로 열린다.
    let (status, _stream) = open_websocket(port, ws_cookie(&good_token));
    assert_eq!(status, 101);
}

#[test]
fn test_static_page_and_security_headers() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    let response = get(gateway.port, "/", &[]);
    assert_eq!(response.status, 200);
    assert!(response.body.contains("Paddock Remote"));
    for header in [
        "Content-Security-Policy: default-src 'self'; frame-ancestors 'none'",
        "X-Content-Type-Options: nosniff",
        "Referrer-Policy: no-referrer",
    ] {
        assert!(response.head.contains(header), "missing {header}:\n{}", response.head);
    }
    // 정적 화면은 쿠키 없이 열리고, 없는 경로는 Host·로그인을 지난 뒤 404다.
    assert_eq!(get(gateway.port, "/missing", &[]).status, 404);
    assert_eq!(get(gateway.port, "/missing", &[("Host", Some("evil".into()))]).status, 403);
    // 에러 응답에도 보안 헤더가 붙는다.
    assert!(get(gateway.port, "/", &[("Host", None)]).head.contains("nosniff"));
}

#[test]
fn test_pairing_issues_a_cookie_once_and_the_device_can_open_a_websocket_until_revoked() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    assert!(!gateway.status().pairing_open);
    let opened = gateway.open_pairing();
    assert_eq!(opened.code.len(), 32);
    assert!(opened.expires_in_secs > 290 && opened.expires_in_secs <= 300);
    assert!(gateway.status().pairing_open);

    let wrong = post_pair(gateway.port, &"0".repeat(32), &[]);
    assert_eq!(wrong.status, 403);
    assert!(!wrong.head.contains("Set-Cookie"));
    let accepted = post_pair(gateway.port, &opened.code, &[]);
    assert_eq!(accepted.status, 200, "{}", accepted.head);
    let set_cookie = accepted.head.lines().find(|line| line.starts_with("Set-Cookie: ")).unwrap().to_string();
    for attribute in ["__Host-paddock-device=", "Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
        assert!(set_cookie.contains(attribute), "{set_cookie}");
    }
    assert!(!set_cookie.contains("Domain"));
    let token = cookie_token(&accepted.head);
    assert_eq!(token.len(), 64);
    assert_eq!(post_pair(gateway.port, &opened.code, &[]).status, 403, "the code works once");
    assert!(!gateway.status().pairing_open);

    // 기기 목록에는 이름만 있고 토큰은 없다.
    let devices = gateway.devices().devices;
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Test phone");
    let file = std::fs::read_to_string(home.path().join(".paddock/native/remote/devices.json")).unwrap();
    assert!(!file.contains(&token), "token must not reach the disk");
    assert!(file.contains(&hash_token(&token)));

    // WebSocket: 인사를 받고, 폐기하면 연결이 끊기고, 같은 토큰은 다시 못 쓴다.
    let (status, stream) = open_websocket(gateway.port, cookie_header(&token));
    assert_eq!(status, 101);
    let mut socket = WebSocket::from_raw_socket(stream, Role::Client, None);
    match socket.read().unwrap() {
        Message::Text(text) => {
            let hello: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
            assert_eq!(hello["type"], "hello");
            assert_eq!(hello["device"], "Test phone");
        }
        other => panic!("expected hello, got {other:?}"),
    }
    assert_eq!(gateway.status().n_connections, 1);
    let revoke = Frame::json(kind::REVOKE, &Revoke { id: devices[0].id.clone() });
    let reply = gateway.control(revoke.kind, revoke.payload);
    assert_eq!(reply.kind, kind::OK);
    match socket.read() {
        Ok(Message::Close(_)) | Err(_) => {}
        other => panic!("revoked device's connection must end, got {other:?}"),
    }
    let (status, _) = open_websocket(gateway.port, cookie_header(&token));
    assert_eq!(status, 401);
    assert!(gateway.log_text().contains("denied websocket: token-revoked"));
    assert!(gateway.devices().devices.is_empty());
    let again = gateway.control(kind::REVOKE, serde_json::to_vec(&Revoke { id: devices[0].id.clone() }).unwrap());
    assert_eq!(again.kind, kind::ERR, "revoking twice finds nothing active");
}

#[test]
fn test_window_closes_after_five_wrong_codes() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    let opened = gateway.open_pairing();
    for _ in 0..5 {
        assert_eq!(post_pair(gateway.port, &"f".repeat(32), &[]).status, 403);
    }
    assert!(!gateway.status().pairing_open, "five failures close the window");
    let response = post_pair(gateway.port, &opened.code, &[]);
    assert_eq!(response.status, 403, "even the right code is refused afterwards");
    assert!(!response.head.contains("Set-Cookie"));
    assert!(gateway.devices().devices.is_empty());
    assert!(gateway.log_text().contains("failures 5, window closed: true"));
}

#[test]
fn test_log_never_contains_tokens_codes_or_cookies() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    let opened = gateway.open_pairing();
    let wrong_code = "abababababababababababababababab";
    post_pair(gateway.port, wrong_code, &[]);
    let token = {
        let response = post_pair(gateway.port, &opened.code, &[]);
        cookie_token(&response.head)
    };
    let attacker_cookie = "attacker-cookie-value-123";
    let (status, stream) = open_websocket(gateway.port, cookie_header(&token));
    assert_eq!(status, 101);
    drop(stream);
    open_websocket(gateway.port, cookie_header(attacker_cookie));
    open_websocket(gateway.port, Some(format!("{}={}; other=zzz-session-9", DeviceCookie::NAME, attacker_cookie)));
    post_pair(gateway.port, "x", &[("Tailscale-User-Login", Some("intruder@example.com".into()))]);
    std::thread::sleep(Duration::from_millis(300));
    let log = gateway.log_text();
    assert!(log.contains("pairing succeeded"), "{log}");
    assert!(log.contains("device connected"), "{log}");
    for secret in [
        opened.code.as_str(),
        wrong_code,
        token.as_str(),
        &hash_token(&token),
        attacker_cookie,
        "zzz-session-9",
        "intruder@example.com",
        DeviceCookie::NAME,
    ] {
        assert!(!log.contains(secret), "log must not contain {secret:?}:\n{log}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.path().join(".paddock/native/logs/remote.log")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn test_listens_on_loopback_only() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    assert_eq!(gateway.status().port, gateway.port);
    // 이 기계의 바깥 주소(있다면)로는 접속되지 않아야 한다. 패킷을 보내지 않는 UDP 연결로 바깥 주소를 알아낸다.
    let outward = UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("192.0.2.1:9").map(|()| socket))
        .and_then(|socket| socket.local_addr());
    match outward {
        Ok(address) if !address.ip().is_loopback() && !address.ip().is_unspecified() => {
            let target = SocketAddr::new(address.ip(), gateway.port);
            assert!(
                TcpStream::connect_timeout(&target, Duration::from_secs(2)).is_err(),
                "{target} must not accept connections"
            );
        }
        other => eprintln!("no non-loopback address on this machine ({other:?}); only the listener check below runs"),
    }
    // 듣는 소켓 목록에도 127.0.0.1만 있어야 한다(lsof가 있을 때).
    if let Ok(output) = Command::new("lsof")
        .args(["-nP", "-a", "-p", &gateway.child.id().to_string(), "-iTCP", "-sTCP:LISTEN"])
        .output()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        if !text.trim().is_empty() {
            assert!(text.contains(&format!("127.0.0.1:{}", gateway.port)), "{text}");
            assert!(!text.contains(&format!("*:{}", gateway.port)), "{text}");
        }
    }
}

#[test]
fn test_request_limits() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    // 머리가 8KiB를 넘으면 431이다.
    let big = format!("GET / HTTP/1.1\r\nHost: {HOST}\r\nX-Pad: {}\r\n\r\n", "a".repeat(9000));
    assert_eq!(raw_request(gateway.port, &big).status, 431);
    // 본문이 상한을 넘는다고 알리면 본문을 읽지 않고 413이다.
    let mut headers = headers_with(&[]);
    headers.push(("Content-Length", Some("100000".into())));
    let mut text = String::from("POST /pair HTTP/1.1\r\n");
    for (name, value) in &headers {
        text.push_str(&format!("{name}: {}\r\n", value.as_deref().unwrap_or("")));
    }
    text.push_str("\r\n");
    assert_eq!(raw_request(gateway.port, &text).status, 413);
    // Content-Length가 없는 POST와 깨진 JSON.
    assert_eq!(raw_request(gateway.port, &text.replace("Content-Length: 100000\r\n", "")).status, 411);
    let broken = request_text("POST", "/pair", &headers_with(&[]), "not json");
    assert_eq!(raw_request(gateway.port, &broken).status, 400);
    // 머리를 끝내지 않고 가만히 있으면 시간 제한 뒤 408이다.
    let started = Instant::now();
    let mut slow = TcpStream::connect(("127.0.0.1", gateway.port)).unwrap();
    slow.write_all(b"GET / HTTP/1.1\r\nHost: ").unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut answer = String::new();
    let _ = slow.read_to_string(&mut answer);
    assert!(answer.starts_with("HTTP/1.1 408"), "{answer}");
    assert!(
        started.elapsed() >= Duration::from_secs(4) && started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn test_control_socket_is_owner_only() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let gateway = Gateway::start(home.path(), None);
    let status = gateway.status();
    assert_eq!(status.expected_host, HOST);
    assert_eq!(status.expected_login, LOGIN);
    assert!(status.daemon_compatible);
    assert_eq!(gateway.control(99, Vec::new()).kind, kind::ERR, "unknown request");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(gateway.control_endpoint()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode =
            std::fs::metadata(Path::new(&gateway.control_endpoint()).parent().unwrap()).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }
}

#[test]
fn test_second_gateway_is_refused_while_the_first_runs() {
    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let _gateway = Gateway::start(home.path(), None);
    let output = Gateway::spawn(home.path(), &[]).wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("already running"));
}

#[test]
fn test_gateway_exits_when_the_daemon_connection_drops_and_on_stop() {
    let home = new_home();
    let daemon = FakeDaemon::start(home.path());
    let mut gateway = Gateway::start(home.path(), None);
    daemon.drop_connection();
    assert_eq!(gateway.wait_exit(Duration::from_secs(5)), Some(0), "gateway must exit when the daemon link ends");
    assert!(gateway.log_text().contains("gateway stopped: daemon connection lost"));
    assert!(!Path::new(&gateway.control_endpoint()).exists(), "control socket file is removed");

    let home = new_home();
    let _daemon = FakeDaemon::start(home.path());
    let mut gateway = Gateway::start(home.path(), None);
    assert_eq!(gateway.control(kind::STOP, Vec::new()).kind, kind::OK);
    assert_eq!(gateway.wait_exit(Duration::from_secs(5)), Some(0));
    assert!(gateway.log_text().contains("gateway stopped: stop requested"));
}

#[test]
fn test_gateway_does_not_start_without_a_daemon() {
    let home = new_home();
    let output = Gateway::spawn(home.path(), &[]).wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("daemon is not running"));
}
