//! 데몬 실행 파일의 지문. 앱이 업데이트돼 데몬이 낡았는지를 가른다.
use std::fs::File;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::message::PROTOCOL_VERSION;

/// 실행 파일 전체의 SHA-256에 프로토콜 버전을 더한 16진수 지문(64자).
///
/// 파일 내용이나 프로토콜 버전이 하나라도 다르면 지문이 다르다. 파일을 읽지 못하면 그 오류다.
pub fn fingerprint_of(exe: &Path) -> io::Result<String> {
    let mut file = File::open(exe)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    hasher.update(PROTOCOL_VERSION.to_le_bytes());
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_is_stable_hex_and_follows_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.exe");
        let b = dir.path().join("b.exe");
        std::fs::write(&a, b"daemon build 1").unwrap();
        std::fs::write(&b, b"daemon build 2").unwrap();
        let first = fingerprint_of(&a).unwrap();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(first, fingerprint_of(&a).unwrap());
        assert_ne!(first, fingerprint_of(&b).unwrap());
    }

    #[test]
    fn test_fingerprint_includes_protocol_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d");
        std::fs::write(&path, b"same bytes").unwrap();
        let bare = {
            use sha2::{Digest, Sha256};
            Sha256::digest(b"same bytes").iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert_ne!(fingerprint_of(&path).unwrap(), bare);
    }

    #[test]
    fn test_missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(fingerprint_of(&dir.path().join("nope")).is_err());
    }
}
