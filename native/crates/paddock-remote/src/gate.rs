//! 브라우저 요청 방어: 요청 머리가 게이트를 지나는지 순수 계산으로 가린다.
//!
//! 요청마다 아래 순서로 보고, 하나라도 틀리면 본문 없이 거부하며 이유만 돌려준다.
//!
//! ```text
//! 요청 ─► Host == 기대 Host ─► 로그인 헤더 == 이 맥의 로그인 ─► (WebSocket·POST /pair) Origin == https://<기대 Host>
//!      ─► (WebSocket) 기기 토큰 쿠키 ─► 처리
//! ```
//!
//! 정적 화면과 `POST /pair`는 쿠키가 없는 기기도 열어야 하므로 쿠키 검사에서 빠진다. 상태를 바꾸는 길은 WebSocket과 `POST /pair` 둘뿐이다.
//! 로그인 헤더는 `tailscale serve`가 붙이는 값이라 그 자체로는 인증이 아니다(루프백 포트에 직접 접속하면 아무 값이나 보낼 수 있다) —
//! 문은 기기 토큰과 페어링 코드이고, 이 검사는 다른 사용자 신원·악성 페이지·주소 바꿔치기를 앞에서 거르는 겹이다.
//! 입력은 요청 머리·기대 값·기기 목록·현재 시각뿐이라 소켓 없이 시험한다.
use crate::devices::{DeviceCookie, DeviceRecord, TokenState, classify};

/// 게이트가 기대하는 값. 실행 인자로 받는다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateConfig {
    /// `Host` 헤더의 기대 값(`이름:포트`).
    pub expected_host: String,
    /// `Tailscale-User-Login` 헤더의 기대 값(이 맥의 로그인).
    pub expected_login: String,
}

/// 게이트가 보는 요청 머리. 해석(`http.rs`)이 채운다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    /// 쿼리를 뺀 경로.
    pub path: String,
    pub host: Option<String>,
    pub login: Option<String>,
    pub origin: Option<String>,
    pub cookie: Option<String>,
    /// `Upgrade: websocket`과 `Connection: upgrade`가 함께 있는가.
    pub is_websocket_upgrade: bool,
    /// 한 번만 와야 하는 헤더(Host·로그인·Origin·Cookie)가 두 번 이상 온 경우 그 이름.
    pub duplicate_header: Option<&'static str>,
}

/// 요청이 가려는 곳.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 정적 화면(`GET /`).
    Static,
    /// 기기 페어링(`POST /pair`).
    Pair,
    /// 기기 WebSocket(`GET /ws` + 승격).
    WebSocket,
    /// 어느 길도 아니다. Host·로그인을 지난 뒤 404다.
    Unknown,
}

impl Route {
    /// 로그에 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Route::Static => "static",
            Route::Pair => "pair",
            Route::WebSocket => "websocket",
            Route::Unknown => "unknown",
        }
    }
}

/// 요청 머리가 어느 길로 가는지 가른다.
///
/// Examples
/// --------
/// | 메서드·경로                        | 결과        |
/// |------------------------------------|-------------|
/// | `GET /` · `GET /index.html`        | `Static`    |
/// | `POST /pair`                       | `Pair`      |
/// | `GET /ws` + WebSocket 승격         | `WebSocket` |
/// | `GET /ws`(승격 없음) · 그 밖 전부  | `Unknown`   |
pub fn route_of(head: &RequestHead) -> Route {
    match (head.method.as_str(), head.path.as_str()) {
        ("GET", "/" | "/index.html") => Route::Static,
        ("POST", "/pair") => Route::Pair,
        ("GET", "/ws") if head.is_websocket_upgrade => Route::WebSocket,
        _ => Route::Unknown,
    }
}

/// 요청을 거부하는 이유. 이름은 기록에 남고, 응답은 본문 없이 상태 코드만 나간다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Denial {
    DuplicateHeader(&'static str),
    HostMismatch,
    LoginMissing,
    LoginMismatch,
    OriginMissing,
    OriginMismatch,
    TokenMissing,
    TokenUnknown,
    TokenRevoked,
    TokenExpired,
}

impl Denial {
    /// 기록에 남기는 이유 이름.
    pub fn reason(&self) -> String {
        match self {
            Denial::DuplicateHeader(name) => format!("duplicate-{name}-header"),
            Denial::HostMismatch => "host-mismatch".to_string(),
            Denial::LoginMissing => "login-missing".to_string(),
            Denial::LoginMismatch => "login-mismatch".to_string(),
            Denial::OriginMissing => "origin-missing".to_string(),
            Denial::OriginMismatch => "origin-mismatch".to_string(),
            Denial::TokenMissing => "token-missing".to_string(),
            Denial::TokenUnknown => "token-unknown".to_string(),
            Denial::TokenRevoked => "token-revoked".to_string(),
            Denial::TokenExpired => "token-expired".to_string(),
        }
    }

    /// 응답 상태 코드. 기기 토큰이 문제일 때만 401이라 화면이 "다시 페어링"을 안내할 수 있고, 나머지는 403이다.
    pub fn status(&self) -> u16 {
        match self {
            Denial::TokenMissing | Denial::TokenUnknown | Denial::TokenRevoked | Denial::TokenExpired => 401,
            _ => 403,
        }
    }
}

/// 게이트를 지난 요청.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub route: Route,
    /// 기기 토큰으로 확인된 기기(번호와 이름). 쿠키 검사를 안 하는 길에서는 `None`이다.
    pub device: Option<(String, String)>,
}

/// 요청 머리를 게이트에 통과시킨다. 위 도식의 순서대로 보고 처음 틀린 곳의 이유를 돌려준다.
///
/// Examples
/// --------
/// | 요청                                       | 결과                               |
/// |--------------------------------------------|------------------------------------|
/// | 모두 맞는 `GET /`                          | `Static`, 기기 없음                |
/// | Host가 기대와 다름                         | `HostMismatch`                     |
/// | 로그인 헤더 없음 / 다른 로그인             | `LoginMissing` / `LoginMismatch`   |
/// | `POST /pair`에 Origin 없음·다름            | `OriginMissing` / `OriginMismatch` |
/// | `GET /ws`에 쿠키 없음                      | `TokenMissing`                     |
/// | `GET /ws`의 토큰이 폐기·만료·모르는 값     | `TokenRevoked`·`TokenExpired`·`TokenUnknown` |
/// | 모두 맞는 `GET /ws`                        | `WebSocket`, 기기 있음             |
pub fn check(
    head: &RequestHead,
    config: &GateConfig,
    devices: &[DeviceRecord],
    now: u64,
) -> Result<Admission, Denial> {
    check_host_and_login(head, config)?;
    let route = route_of(head);
    if matches!(route, Route::Pair | Route::WebSocket) {
        check_origin(head, config)?;
    }
    let device = if route == Route::WebSocket { Some(check_token(head, devices, now)?) } else { None };
    Ok(Admission { route, device })
}

/// 중복 헤더·Host·로그인 헤더를 순서대로 본다.
fn check_host_and_login(
    head: &RequestHead,
    config: &GateConfig,
) -> Result<(), Denial> {
    if let Some(name) = head.duplicate_header {
        Err(Denial::DuplicateHeader(name))
    } else if head.host.as_deref() != Some(config.expected_host.as_str()) {
        Err(Denial::HostMismatch)
    } else {
        match head.login.as_deref() {
            None => Err(Denial::LoginMissing),
            Some(login) if login != config.expected_login => Err(Denial::LoginMismatch),
            Some(_) => Ok(()),
        }
    }
}

/// Origin이 `https://<기대 Host>`와 정확히 같은지 본다.
fn check_origin(
    head: &RequestHead,
    config: &GateConfig,
) -> Result<(), Denial> {
    match head.origin.as_deref() {
        None => Err(Denial::OriginMissing),
        Some(origin) if origin != format!("https://{}", config.expected_host) => Err(Denial::OriginMismatch),
        Some(_) => Ok(()),
    }
}

/// 기기 토큰 쿠키를 기기 목록에 대 보고, 맞으면 기기의 번호와 이름을 돌려준다.
fn check_token(
    head: &RequestHead,
    devices: &[DeviceRecord],
    now: u64,
) -> Result<(String, String), Denial> {
    let token = head
        .cookie
        .as_deref()
        .and_then(|cookie| cookie_value(cookie, DeviceCookie::NAME))
        .ok_or(Denial::TokenMissing)?;
    match classify(devices, token, now) {
        TokenState::Valid { id, name } => Ok((id, name)),
        TokenState::Unknown => Err(Denial::TokenUnknown),
        TokenState::Revoked => Err(Denial::TokenRevoked),
        TokenState::Expired => Err(Denial::TokenExpired),
    }
}

/// `Cookie` 헤더에서 `name`의 값을 꺼낸다. 같은 이름이 여럿이면 첫 값이다.
///
/// Examples
/// --------
/// | Cookie 헤더            | name | 결과        |
/// |------------------------|------|-------------|
/// | `a=1; b=2`             | `b`  | `Some("2")` |
/// | `a=1`                  | `b`  | `None`      |
/// | `ab=1`                 | `b`  | `None`      |
pub fn cookie_value<'a>(
    header: &'a str,
    name: &str,
) -> Option<&'a str> {
    header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Lifetime, hash_token};

    const NOW: u64 = 1_800_000_000;
    const HOST: &str = "mac.tail1234.ts.net:8443";
    const LOGIN: &str = "me@example.com";

    fn config() -> GateConfig {
        GateConfig { expected_host: HOST.into(), expected_login: LOGIN.into() }
    }

    fn device(
        token: &str,
        last_seen: u64,
        revoked: Option<u64>,
    ) -> DeviceRecord {
        DeviceRecord {
            id: "d1".into(),
            name: "Phone".into(),
            token_hash: hash_token(token),
            created: last_seen,
            last_seen,
            revoked,
        }
    }

    /// 모든 검사를 통과하는 요청 머리에서 시작한다.
    fn good(route: Route) -> RequestHead {
        let mut head = RequestHead { host: Some(HOST.into()), login: Some(LOGIN.into()), ..RequestHead::default() };
        match route {
            Route::Static => {
                head.method = "GET".into();
                head.path = "/".into();
            }
            Route::Pair => {
                head.method = "POST".into();
                head.path = "/pair".into();
                head.origin = Some(format!("https://{HOST}"));
            }
            Route::WebSocket => {
                head.method = "GET".into();
                head.path = "/ws".into();
                head.is_websocket_upgrade = true;
                head.origin = Some(format!("https://{HOST}"));
                head.cookie = Some(format!("{}=goodtoken", DeviceCookie::NAME));
            }
            Route::Unknown => {
                head.method = "GET".into();
                head.path = "/nope".into();
            }
        }
        head
    }

    fn devices() -> Vec<DeviceRecord> {
        vec![device("goodtoken", NOW - 60, None)]
    }

    fn denial_of(head: &RequestHead) -> Denial {
        check(head, &config(), &devices(), NOW).unwrap_err()
    }

    #[test]
    fn test_requests_that_pass_every_check() {
        for route in [Route::Static, Route::Pair, Route::Unknown] {
            let admission = check(&good(route), &config(), &devices(), NOW).unwrap();
            assert_eq!(admission, Admission { route, device: None });
        }
        let admission = check(&good(Route::WebSocket), &config(), &devices(), NOW).unwrap();
        assert_eq!(admission, Admission { route: Route::WebSocket, device: Some(("d1".into(), "Phone".into())) });
    }

    #[test]
    fn test_host_mismatch_is_denied_on_every_route() {
        for route in [Route::Static, Route::Pair, Route::WebSocket, Route::Unknown] {
            for wrong in [
                Some("evil.example:8443"),
                Some("mac.tail1234.ts.net"),
                Some("MAC.tail1234.ts.net:8443"),
                Some(""),
                None,
            ] {
                let mut head = good(route);
                head.host = wrong.map(str::to_string);
                assert_eq!(denial_of(&head), Denial::HostMismatch, "{route:?} {wrong:?}");
            }
        }
    }

    #[test]
    fn test_login_header_missing_or_different_is_denied() {
        for route in [Route::Static, Route::Pair, Route::WebSocket] {
            let mut head = good(route);
            head.login = None;
            assert_eq!(denial_of(&head), Denial::LoginMissing, "{route:?}");
            head.login = Some("other@example.com".into());
            assert_eq!(denial_of(&head), Denial::LoginMismatch, "{route:?}");
            head.login = Some(String::new());
            assert_eq!(denial_of(&head), Denial::LoginMismatch, "{route:?}");
        }
    }

    #[test]
    fn test_origin_is_required_and_exact_for_the_state_changing_routes_only() {
        for route in [Route::Pair, Route::WebSocket] {
            let mut head = good(route);
            head.origin = None;
            assert_eq!(denial_of(&head), Denial::OriginMissing, "{route:?}");
            for wrong in [
                "https://evil.example",
                "http://mac.tail1234.ts.net:8443",
                "https://mac.tail1234.ts.net",
                "null",
                "https://mac.tail1234.ts.net:8443/",
            ] {
                head.origin = Some(wrong.into());
                assert_eq!(denial_of(&head), Denial::OriginMismatch, "{route:?} {wrong}");
            }
        }
        // 정적 화면은 Origin을 보지 않는다(주소창 이동에는 Origin이 없다).
        let mut head = good(Route::Static);
        head.origin = Some("https://evil.example".into());
        assert!(check(&head, &config(), &devices(), NOW).is_ok());
    }

    #[test]
    fn test_websocket_needs_a_valid_device_token() {
        let mut head = good(Route::WebSocket);
        head.cookie = None;
        assert_eq!(denial_of(&head), Denial::TokenMissing);
        head.cookie = Some("other=1".into());
        assert_eq!(denial_of(&head), Denial::TokenMissing, "cookie of another name");
        head.cookie = Some(format!("{}=nottherightone", DeviceCookie::NAME));
        assert_eq!(denial_of(&head), Denial::TokenUnknown);
        // 이름 접두가 없는 쿠키는 장치 쿠키로 보지 않는다.
        head.cookie = Some("paddock-device=goodtoken".into());
        assert_eq!(denial_of(&head), Denial::TokenMissing);
    }

    #[test]
    fn test_revoked_and_expired_tokens_are_denied_with_their_own_reason() {
        let head = good(Route::WebSocket);
        let revoked = vec![device("goodtoken", NOW - 60, Some(NOW - 30))];
        assert_eq!(check(&head, &config(), &revoked, NOW).unwrap_err(), Denial::TokenRevoked);
        let expired = vec![device("goodtoken", NOW - Lifetime::IDLE_EXPIRY_SECS - 1, None)];
        assert_eq!(check(&head, &config(), &expired, NOW).unwrap_err(), Denial::TokenExpired);
    }

    #[test]
    fn test_check_order_reports_the_first_failing_layer() {
        // 모든 층이 틀리면 Host가 먼저다.
        let mut head = good(Route::WebSocket);
        head.host = None;
        head.login = None;
        head.origin = None;
        head.cookie = None;
        assert_eq!(denial_of(&head), Denial::HostMismatch);
        head.host = Some(HOST.into());
        assert_eq!(denial_of(&head), Denial::LoginMissing);
        head.login = Some(LOGIN.into());
        assert_eq!(denial_of(&head), Denial::OriginMissing);
        head.origin = Some(format!("https://{HOST}"));
        assert_eq!(denial_of(&head), Denial::TokenMissing);
    }

    #[test]
    fn test_duplicate_headers_are_denied_first() {
        let mut head = good(Route::Static);
        head.duplicate_header = Some("host");
        assert_eq!(denial_of(&head), Denial::DuplicateHeader("host"));
    }

    #[test]
    fn test_route_rules() {
        let mut head = good(Route::WebSocket);
        head.is_websocket_upgrade = false;
        assert_eq!(route_of(&head), Route::Unknown);
        head = good(Route::Static);
        head.method = "POST".into();
        assert_eq!(route_of(&head), Route::Unknown, "POST / is not a static request");
        head = good(Route::Pair);
        head.method = "GET".into();
        assert_eq!(route_of(&head), Route::Unknown);
        head.path = "/index.html".into();
        assert_eq!(route_of(&head), Route::Static);
    }

    #[test]
    fn test_denial_status_and_reason() {
        assert_eq!(Denial::HostMismatch.status(), 403);
        assert_eq!(Denial::OriginMismatch.status(), 403);
        assert_eq!(Denial::TokenRevoked.status(), 401);
        assert_eq!(Denial::TokenMissing.reason(), "token-missing");
        assert_eq!(Denial::DuplicateHeader("host").reason(), "duplicate-host-header");
    }

    #[test]
    fn test_cookie_value() {
        assert_eq!(cookie_value("a=1; b=2", "b"), Some("2"));
        assert_eq!(cookie_value("a=1", "b"), None);
        assert_eq!(cookie_value("ab=1", "b"), None);
        assert_eq!(cookie_value("b=1; b=2", "b"), Some("1"));
        assert_eq!(cookie_value("", "b"), None);
    }
}
