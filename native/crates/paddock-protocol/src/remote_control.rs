//! 앱↔원격 게이트웨이 제어 메시지: 프레임 종류 번호와 JSON 내용의 구조체.
//!
//! 앱이 게이트웨이의 상태를 읽고, 기기 승인 창을 열고, 기기 목록을 읽고, 기기를 폐기하고, 게이트웨이를 끝내는 데 쓴다.
//! 데몬 프로토콜(`message`)과 별개의 연결·별개의 번호 공간이라 `PROTOCOL_VERSION`과 무관하다 — 앱과 게이트웨이는 한 묶음으로 배포돼 판이 같다.
//! 연결은 소유자 전용 소켓이고(`endpoint::remote_control_endpoint_name`) 게이트웨이가 접속자의 사용자를 확인하므로 토큰 인사는 없다.
//! 프레임 모양은 데몬 프로토콜과 같다(`frame::Frame`). 요청마다 답 하나가 있고, 실패는 모두 `ERR`(글자)다.
use serde::{Deserialize, Serialize};

/// 제어 프레임 종류 번호. 1~99는 앱 → 게이트웨이, 100 이상은 게이트웨이 → 앱이다.
pub mod kind {
    // -- 앱 -> 게이트웨이 --
    /// 내용 없음. `STATUS_REPLY`로 답한다.
    pub const STATUS: u8 = 1;
    /// 내용 없음. 기기 승인 창을 새로 열고(열려 있던 창은 닫힌다) `PAIRING_OPENED`로 답한다.
    pub const OPEN_PAIRING: u8 = 2;
    /// 내용 없음. `DEVICES_REPLY`로 답한다.
    pub const DEVICES: u8 = 3;
    /// `Revoke` JSON. 기기를 폐기하고 그 기기의 열린 연결을 끊은 뒤 `OK`로 답한다. 없는 기기면 `ERR`다.
    pub const REVOKE: u8 = 4;
    /// 내용 없음. `OK`로 답한 뒤 게이트웨이가 끝난다.
    pub const STOP: u8 = 5;

    // -- 게이트웨이 -> 앱 --
    /// `GatewayStatus` JSON.
    pub const STATUS_REPLY: u8 = 101;
    /// `PairingOpened` JSON.
    pub const PAIRING_OPENED: u8 = 102;
    /// `DevicesReply` JSON.
    pub const DEVICES_REPLY: u8 = 103;
    /// 내용 없음. `REVOKE`·`STOP`이 받아들여졌다는 답.
    pub const OK: u8 = 104;
    /// 오류 문장(글자).
    pub const ERR: u8 = 105;
}

/// 게이트웨이의 지금 상태.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub pid: u32,
    /// 127.0.0.1에서 듣는 포트.
    pub port: u16,
    /// 접속을 받는 기대 Host(`이름:포트`)와 이 맥의 로그인. 둘이 맞는 요청만 게이트를 지난다.
    pub expected_host: String,
    pub expected_login: String,
    /// 데몬에 붙어 있고 데몬의 프로토콜 버전이 게이트웨이와 같은가. 거짓이면 터미널을 잇지 못한다.
    pub daemon_compatible: bool,
    /// 폐기·만료되지 않은 기기 수.
    pub n_devices: u32,
    /// 지금 열려 있는 기기 WebSocket 수.
    pub n_connections: u32,
    /// 기기 승인 창이 열려 있는가.
    pub pairing_open: bool,
    /// 가장 최근에 열었던 창이 닫힌 이유(`accepted`·`too-many-failures`·`expired`). 열려 있거나 연 적이 없으면 없다.
    /// 앱은 `too-many-failures`를 보고 사용자에게 창을 다시 열자고 안내한다.
    #[serde(default)]
    pub pairing_closed_reason: Option<String>,
}

/// 새로 연 기기 승인 창. 코드는 이 응답으로만 나가고 디스크에는 닿지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingOpened {
    /// 1회용 128비트 코드(16진수 32자).
    pub code: String,
    /// 창이 닫히기까지 남은 초.
    pub expires_in_secs: u64,
}

/// 페어링한 기기 한 대의 표시용 정보. 토큰이나 그 해시는 담지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// 폐기할 때 가리키는 무작위 번호(토큰과 무관).
    pub id: String,
    pub name: String,
    /// 페어링한 시각과 마지막으로 쓴 시각(유닉스 초).
    pub created: u64,
    pub last_seen: u64,
}

/// 폐기·만료되지 않은 기기 목록.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesReply {
    pub devices: Vec<DeviceInfo>,
}

/// 폐기할 기기.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoke {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;

    #[test]
    fn test_kind_numbers_are_unique_and_split_by_direction() {
        let requests = [kind::STATUS, kind::OPEN_PAIRING, kind::DEVICES, kind::REVOKE, kind::STOP];
        let replies = [kind::STATUS_REPLY, kind::PAIRING_OPENED, kind::DEVICES_REPLY, kind::OK, kind::ERR];
        let mut all: Vec<u8> = requests.iter().chain(replies.iter()).copied().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), requests.len() + replies.len());
        assert!(requests.iter().all(|kind| *kind < 100));
        assert!(replies.iter().all(|kind| *kind >= 100));
    }

    #[test]
    fn test_messages_roundtrip_through_frames() {
        let status = GatewayStatus {
            pid: 7,
            port: 9000,
            expected_host: "mac.tail.ts.net:8443".into(),
            expected_login: "me@example.com".into(),
            daemon_compatible: true,
            n_devices: 2,
            n_connections: 1,
            pairing_open: false,
            pairing_closed_reason: Some("expired".into()),
        };
        let frame = Frame::json(kind::STATUS_REPLY, &status);
        assert_eq!(frame.parse_json::<GatewayStatus>().unwrap(), status);
        let devices = DevicesReply {
            devices: vec![DeviceInfo { id: "ab".into(), name: "Phone".into(), created: 1, last_seen: 2 }],
        };
        assert_eq!(Frame::json(kind::DEVICES_REPLY, &devices).parse_json::<DevicesReply>().unwrap(), devices);
    }
}
