//! 시험 전용 환경 변수. 앱 시험이 다른 버전의 데몬을 흉내 낼 때 쓴다.
//!
//! cargo 기능 `test-hooks`를 켠 빌드에서만 읽는다. 기능이 꺼진 빌드(배포 빌드 포함)는 환경 변수를 읽지 않고 항상 실제 값을 쓴다.
use paddock_protocol::message::PROTOCOL_VERSION;

/// 인사에 쓰는 프로토콜 버전. `PADDOCK_DAEMON_PROTOCOL_VERSION=<숫자>`가 있으면 그 값이다(앱의 버전과 비교하는 값도 이것이다).
#[cfg(feature = "test-hooks")]
pub fn protocol_version() -> u32 {
    let hooked = std::env::var("PADDOCK_DAEMON_PROTOCOL_VERSION").ok().and_then(|value| value.parse().ok());
    let version = hooked.unwrap_or(PROTOCOL_VERSION);
    if version != PROTOCOL_VERSION {
        crate::log::event(&format!("test hook: greeting with protocol v{version} (built v{PROTOCOL_VERSION})"));
    }
    version
}

#[cfg(not(feature = "test-hooks"))]
pub fn protocol_version() -> u32 {
    PROTOCOL_VERSION
}

/// 참이면 버전이 다른 인사를 `ERR` 뒤 연결 닫기로 거절한다(인사의 버전을 따지지 않던 옛 데몬). `PADDOCK_DAEMON_STRICT_HELLO=1`.
#[cfg(feature = "test-hooks")]
pub fn strict_hello() -> bool {
    let strict = std::env::var("PADDOCK_DAEMON_STRICT_HELLO").is_ok_and(|value| value == "1");
    if strict {
        crate::log::event(
            "test hook: strict hello (a different protocol version is refused and the connection closed)",
        );
    }
    strict
}

#[cfg(not(feature = "test-hooks"))]
pub fn strict_hello() -> bool {
    false
}
