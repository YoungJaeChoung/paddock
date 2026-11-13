//! 페어링한 기기 목록(`~/.paddock/native/remote/devices.json`)과 기기 토큰 검사.
//!
//! 기기 토큰은 브라우저 쿠키에만 있고 이 맥에는 SHA-256 해시만 남는다. 파일이 새어도 토큰은 복원되지 않는다.
//! 파일은 소유자만 읽고 쓰며(0600), 쓰는 주체는 게이트웨이 하나다(앱은 제어 소켓으로만 목록을 읽고 폐기한다).
//! 토큰은 30일 쓰지 않으면 만료하고, 폐기한 기기는 "폐기됨"이라는 거부 이유를 남기려고 30일 동안 표식으로 둔다.
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use paddock_protocol::daemon_info::{ensure_private_dir, new_token, write_private_file};
use paddock_protocol::remote_control::DeviceInfo;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 토큰 수명 규칙(초).
pub struct Lifetime;

impl Lifetime {
    /// 마지막으로 쓴 지 이 시간이 지난 토큰은 만료한다(30일).
    pub const IDLE_EXPIRY_SECS: u64 = 30 * 24 * 3600;
    /// 폐기한 기기의 표식을 남기는 시간(30일). 지나면 "알 수 없는 토큰"과 구별되지 않는다.
    pub const REVOKED_KEEP_SECS: u64 = 30 * 24 * 3600;
    /// 쿠키의 브라우저 보관 기간(400일, 브라우저가 허용하는 최대 수준). 기기 만료는 서버의 `last_seen`이 정하므로 쿠키는 그보다 오래 둔다 —
    /// 쿠키 쪽이 먼저 사라져 매일 쓰는 기기가 30일째에 버려지는 일을 막는다.
    pub const COOKIE_MAX_AGE_SECS: u64 = 400 * 24 * 3600;
    /// 마지막 사용 시각을 파일에 다시 쓰는 최소 간격(1시간). 요청마다 파일을 쓰지 않으려는 값이다.
    pub const TOUCH_INTERVAL_SECS: u64 = 3600;
}

/// 기기 토큰을 담는 쿠키. `__Host-` 접두는 브라우저가 `Secure`·`Path=/`·`Domain` 없음을 강제하게 한다.
pub struct DeviceCookie;

impl DeviceCookie {
    pub const NAME: &'static str = "__Host-paddock-device";

    /// 토큰을 심는 `Set-Cookie` 값. 페이지 스크립트가 읽지 못하고(`HttpOnly`) 다른 사이트의 요청에 실리지 않는다(`SameSite=Strict`).
    pub fn set_header(token: &str) -> String {
        format!(
            "{}={token}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={}",
            Self::NAME,
            Lifetime::COOKIE_MAX_AGE_SECS
        )
    }
}

/// 이름의 최대 글자 수와 이름이 없을 때의 대체 이름.
struct NameRule;

impl NameRule {
    const MAX_CHARS: usize = 48;
    const FALLBACK: &'static str = "Unnamed device";
}

/// 파일에 저장되는 기기 한 대. 토큰 자체는 없고 해시만 있다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    /// 토큰의 SHA-256(16진수 64자).
    pub token_hash: String,
    /// 페어링한 시각, 마지막으로 쓴 시각, 폐기한 시각(유닉스 초).
    pub created: u64,
    pub last_seen: u64,
    #[serde(default)]
    pub revoked: Option<u64>,
}

/// 토큰 하나를 기기 목록에 대 본 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenState {
    Valid {
        id: String,
        name: String,
    },
    /// 어느 기기의 토큰도 아니다.
    Unknown,
    Revoked,
    Expired,
}

/// 토큰의 SHA-256을 16진수 64자로 만든다.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 두 바이트열이 같은가. 어디서 달라지는지와 무관한 시간으로 비교한다.
pub fn constant_time_eq(
    left: &[u8],
    right: &[u8],
) -> bool {
    let mut diff = left.len() ^ right.len();
    for (index, byte) in left.iter().enumerate() {
        diff |= usize::from(byte ^ right.get(index).copied().unwrap_or(0));
    }
    diff == 0
}

/// 토큰을 기기 목록에 대 본다. 순수 계산이라 시각(`now`, 유닉스 초)을 받는다.
///
/// 목록 전체를 같은 방식으로 끝까지 비교하므로 어느 기기에서 맞았는지가 응답 시간에 드러나지 않는다.
///
/// Examples
/// --------
/// | 목록의 기기                       | 결과        |
/// |-----------------------------------|-------------|
/// | 해시가 같고 폐기·만료 아님        | `Valid`     |
/// | 해시가 같고 폐기됨                | `Revoked`   |
/// | 해시가 같고 30일 넘게 안 씀       | `Expired`   |
/// | 같은 해시 없음                    | `Unknown`   |
pub fn classify(
    records: &[DeviceRecord],
    token: &str,
    now: u64,
) -> TokenState {
    let given = hash_token(token);
    let mut found: Option<&DeviceRecord> = None;
    for record in records {
        if constant_time_eq(given.as_bytes(), record.token_hash.as_bytes()) {
            found = Some(record);
        }
    }
    match found {
        None => TokenState::Unknown,
        Some(record) if record.revoked.is_some() => TokenState::Revoked,
        Some(record) if now.saturating_sub(record.last_seen) > Lifetime::IDLE_EXPIRY_SECS => TokenState::Expired,
        Some(record) => TokenState::Valid { id: record.id.clone(), name: record.name.clone() },
    }
}

/// 브라우저가 보낸 기기 이름을 목록에 쓸 수 있게 다듬는다: 제어 문자를 빼고 앞뒤 공백을 자르고 48자로 줄인다. 비면 대체 이름이다.
///
/// Examples
/// --------
/// | 입력              | 결과              |
/// |-------------------|-------------------|
/// | `" Pixel 9 "`     | `Pixel 9`         |
/// | `"a\nb"`          | `ab`              |
/// | `""`              | `Unnamed device`  |
pub fn sanitize_name(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|ch| !ch.is_control()).take(NameRule::MAX_CHARS).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() { NameRule::FALLBACK.to_string() } else { trimmed.to_string() }
}

/// 기기 목록과 그 파일. 변경은 파일에 쓴 뒤에만 메모리에 반영된다.
pub struct Devices {
    path: PathBuf,
    records: Mutex<Vec<DeviceRecord>>,
}

impl Devices {
    /// 파일을 읽어 목록을 연다. 파일이 없으면 빈 목록이다. 폐기 표식과 오래 안 쓴 기기 중 보존 기간이 지난 것은 이때 버린다.
    ///
    /// 파일이 깨졌으면 오류다 — 모르는 채로 빈 목록으로 덮어쓰면 페어링한 기기가 조용히 사라진다.
    pub fn open(
        path: PathBuf,
        now: u64,
    ) -> io::Result<Self> {
        let mut records = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<DevicesFile>(&text).map(|file| file.devices).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("devices.json is unreadable: {error}"))
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        records.retain(|record| match record.revoked {
            Some(revoked) => now.saturating_sub(revoked) <= Lifetime::REVOKED_KEEP_SECS,
            None => now.saturating_sub(record.last_seen) <= 2 * Lifetime::IDLE_EXPIRY_SECS,
        });
        Ok(Self { path, records: Mutex::new(records) })
    }

    /// 목록을 읽기 전용으로 빌려 `read`에 넘긴다. 게이트 검사가 이것으로 목록을 본다.
    pub fn with_records<R>(
        &self,
        read: impl FnOnce(&[DeviceRecord]) -> R,
    ) -> R {
        let records = self.records.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&records)
    }

    /// 새 기기를 등록하고 토큰을 돌려준다. 토큰은 이 반환값에만 있고 해시만 파일에 간다.
    pub fn add(
        &self,
        name: &str,
        now: u64,
    ) -> io::Result<(DeviceInfo, String)> {
        let token = new_token();
        let mut id_bytes = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut id_bytes);
        let record = DeviceRecord {
            id: id_bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
            name: sanitize_name(name),
            token_hash: hash_token(&token),
            created: now,
            last_seen: now,
            revoked: None,
        };
        let info = info_of(&record);
        self.update(|records| records.push(record))?;
        Ok((info, token))
    }

    /// 기기의 마지막 사용 시각을 `now`로 올린다. 직전 기록과 `TOUCH_INTERVAL_SECS`가 안 벌어졌으면 아무것도 쓰지 않는다.
    pub fn touch(
        &self,
        id: &str,
        now: u64,
    ) -> io::Result<()> {
        let is_stale = self.with_records(|records| {
            records
                .iter()
                .any(|record| record.id == id && now.saturating_sub(record.last_seen) >= Lifetime::TOUCH_INTERVAL_SECS)
        });
        if is_stale {
            self.update(|records| {
                if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                    record.last_seen = now;
                }
            })
        } else {
            Ok(())
        }
    }

    /// 기기를 폐기한다. 폐기한 기기가 있었으면 그 이름을, 없었거나 이미 폐기된 기기면 `None`을 돌려준다.
    pub fn revoke(
        &self,
        id: &str,
        now: u64,
    ) -> io::Result<Option<String>> {
        let name = self.with_records(|records| {
            records.iter().find(|record| record.id == id && record.revoked.is_none()).map(|record| record.name.clone())
        });
        if name.is_some() {
            self.update(|records| {
                if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                    record.revoked = Some(now);
                }
            })?;
        }
        Ok(name)
    }

    /// 기기가 지금 폐기·만료되지 않았는가. 없는 기기는 `false`다.
    pub fn is_active(
        &self,
        id: &str,
        now: u64,
    ) -> bool {
        self.with_records(|records| {
            records.iter().any(|record| {
                record.id == id
                    && record.revoked.is_none()
                    && now.saturating_sub(record.last_seen) <= Lifetime::IDLE_EXPIRY_SECS
            })
        })
    }

    /// 폐기·만료되지 않은 기기를 표시용으로 돌려준다.
    pub fn active(
        &self,
        now: u64,
    ) -> Vec<DeviceInfo> {
        self.with_records(|records| {
            records
                .iter()
                .filter(|record| {
                    record.revoked.is_none() && now.saturating_sub(record.last_seen) <= Lifetime::IDLE_EXPIRY_SECS
                })
                .map(info_of)
                .collect()
        })
    }

    /// 목록을 복사해 `change`로 고친 뒤 파일에 쓰고, 쓰기에 성공했을 때만 메모리 목록을 바꾼다.
    fn update(
        &self,
        change: impl FnOnce(&mut Vec<DeviceRecord>),
    ) -> io::Result<()> {
        let mut records = self.records.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut next = records.clone();
        change(&mut next);
        if let Some(dir) = self.path.parent() {
            ensure_private_dir(dir)?;
        }
        let text = serde_json::to_string_pretty(&DevicesFile { version: 1, devices: next.clone() })
            .expect("device records serialize to JSON");
        write_private_file(&self.path, text.as_bytes())?;
        *records = next;
        Ok(())
    }
}

/// 파일 전체의 모양.
#[derive(Serialize, Deserialize)]
struct DevicesFile {
    version: u32,
    devices: Vec<DeviceRecord>,
}

fn info_of(record: &DeviceRecord) -> DeviceInfo {
    DeviceInfo {
        id: record.id.clone(),
        name: record.name.clone(),
        created: record.created,
        last_seen: record.last_seen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn record(
        token: &str,
        last_seen: u64,
        revoked: Option<u64>,
    ) -> DeviceRecord {
        DeviceRecord {
            id: "id1".into(),
            name: "Phone".into(),
            token_hash: hash_token(token),
            created: last_seen,
            last_seen,
            revoked,
        }
    }

    #[test]
    fn test_classify_covers_every_token_state() {
        let records = [
            record("good", NOW - 10, None),
            record("gone", NOW - 10, Some(NOW - 5)),
            record("old", NOW - Lifetime::IDLE_EXPIRY_SECS - 1, None),
        ];
        assert_eq!(classify(&records, "good", NOW), TokenState::Valid { id: "id1".into(), name: "Phone".into() });
        assert_eq!(classify(&records, "gone", NOW), TokenState::Revoked);
        assert_eq!(classify(&records, "old", NOW), TokenState::Expired);
        assert_eq!(classify(&records, "other", NOW), TokenState::Unknown);
        assert_eq!(classify(&[], "good", NOW), TokenState::Unknown);
        // 만료 경계: 정확히 30일째까지는 유효하다.
        let edge = [record("edge", NOW - Lifetime::IDLE_EXPIRY_SECS, None)];
        assert!(matches!(classify(&edge, "edge", NOW), TokenState::Valid { .. }));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name(" Pixel 9 "), "Pixel 9");
        assert_eq!(sanitize_name("a\nb\u{7}"), "ab");
        assert_eq!(sanitize_name(""), "Unnamed device");
        assert_eq!(sanitize_name("  \n "), "Unnamed device");
        assert_eq!(sanitize_name(&"가".repeat(100)).chars().count(), 48);
    }

    #[test]
    fn test_cookie_header_has_the_required_attributes() {
        let header = DeviceCookie::set_header("tok");
        assert!(header.starts_with("__Host-paddock-device=tok;"));
        for attribute in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
            assert!(header.contains(attribute), "{header}");
        }
        assert!(!header.contains("Domain"));
    }

    #[test]
    fn test_added_device_is_stored_as_a_hash_only_with_owner_only_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote").join("devices.json");
        let devices = Devices::open(path.clone(), NOW).unwrap();
        let (info, token) = devices.add("Phone", NOW).unwrap();
        assert_eq!(token.len(), 64);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&token), "token must not reach the disk");
        assert!(text.contains(&hash_token(&token)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(
            devices.with_records(|records| classify(records, &token, NOW)),
            TokenState::Valid { id: info.id, name: "Phone".into() }
        );
    }

    #[test]
    fn test_revoke_survives_reopen_and_is_listed_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let devices = Devices::open(path.clone(), NOW).unwrap();
        let (info, token) = devices.add("Phone", NOW).unwrap();
        assert_eq!(devices.active(NOW).len(), 1);
        assert_eq!(devices.revoke(&info.id, NOW + 1).unwrap(), Some("Phone".into()));
        assert_eq!(devices.revoke(&info.id, NOW + 2).unwrap(), None, "second revoke finds nothing active");
        assert_eq!(devices.revoke("missing", NOW + 2).unwrap(), None);
        assert!(devices.active(NOW + 2).is_empty());
        let reopened = Devices::open(path, NOW + 3).unwrap();
        assert_eq!(reopened.with_records(|records| classify(records, &token, NOW + 3)), TokenState::Revoked);
    }

    #[test]
    fn test_is_active_follows_revocation_and_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let devices = Devices::open(dir.path().join("devices.json"), NOW).unwrap();
        let (info, _) = devices.add("Phone", NOW).unwrap();
        assert!(devices.is_active(&info.id, NOW));
        assert!(!devices.is_active(&info.id, NOW + Lifetime::IDLE_EXPIRY_SECS + 1));
        assert!(!devices.is_active("missing", NOW));
        devices.revoke(&info.id, NOW).unwrap();
        assert!(!devices.is_active(&info.id, NOW));
    }

    #[test]
    fn test_cookie_outlives_the_server_side_expiry() {
        const { assert!(Lifetime::COOKIE_MAX_AGE_SECS > Lifetime::IDLE_EXPIRY_SECS) };
        assert!(DeviceCookie::set_header("t").contains(&format!("Max-Age={}", Lifetime::COOKIE_MAX_AGE_SECS)));
    }

    #[test]
    fn test_touch_writes_only_after_the_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let devices = Devices::open(path, NOW).unwrap();
        let (info, _) = devices.add("Phone", NOW).unwrap();
        devices.touch(&info.id, NOW + 10).unwrap();
        assert_eq!(devices.active(NOW + 10)[0].last_seen, NOW);
        devices.touch(&info.id, NOW + Lifetime::TOUCH_INTERVAL_SECS).unwrap();
        assert_eq!(
            devices.active(NOW + Lifetime::TOUCH_INTERVAL_SECS)[0].last_seen,
            NOW + Lifetime::TOUCH_INTERVAL_SECS
        );
    }

    #[test]
    fn test_open_prunes_old_revoked_marks_and_rejects_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let devices = Devices::open(path.clone(), NOW).unwrap();
        let (info, token) = devices.add("Phone", NOW).unwrap();
        devices.revoke(&info.id, NOW).unwrap();
        let later = NOW + Lifetime::REVOKED_KEEP_SECS + 1;
        let reopened = Devices::open(path.clone(), later).unwrap();
        assert_eq!(reopened.with_records(|records| classify(records, &token, later)), TokenState::Unknown);
        std::fs::write(&path, "not json").unwrap();
        assert!(Devices::open(path, NOW).is_err());
    }
}
