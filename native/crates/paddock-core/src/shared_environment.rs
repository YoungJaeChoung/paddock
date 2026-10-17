//! 앱이 새 터미널에 넘기는 환경 변수를 고르는 순수 규칙.
//!
//! 데몬은 앱보다 오래 살아 자기 환경이 낡을 수 있다. 앱이 다시 붙을 때 이 목록의 변수만 실어 보내
//! 새 터미널이 앱의 최신 PATH·계정 폴더·프록시 설정을 받게 한다. 이미 열린 터미널의 환경은 바뀌지 않는다.

/// 새 터미널에 넘길 환경 변수 이름. 앱을 다시 열 때 바뀌었을 수 있는 것만 둔다(Windows의 `Path`와 Unix의 `PATH`를 모두 싣는다).
pub const SHARED_NAMES: [&str; 12] = [
    "PATH",
    "Path",
    "PATHEXT",
    "WSLENV",
    "SSH_AUTH_SOCK",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
];

/// 환경 변수 목록에서 `SHARED_NAMES`에 있는 이름만 골라 이름 순서대로 돌려준다. 이름이 목록에 없으면 버린다.
///
/// Examples
/// --------
/// | 입력                                     | 결과                       |
/// |------------------------------------------|----------------------------|
/// | `PATH=/bin`, `SECRET=x`                  | `PATH=/bin`                |
/// | `Path=C:\a`, `PATH=/bin`                 | `PATH=/bin`, `Path=C:\a`   |
/// | (빈 목록)                                | (빈 목록)                  |
pub fn shared_environment(env: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    let env: Vec<(String, String)> = env.into_iter().collect();
    let mut shared = Vec::new();
    for name in SHARED_NAMES {
        if let Some((_, value)) = env.iter().find(|(key, _)| key == name) {
            shared.push((name.to_string(), value.clone()));
        }
    }
    shared
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
    }

    #[test]
    fn test_c_d7_only_allowed_names_are_shared() {
        let shared = shared_environment(vars(&[("PATH", "/bin"), ("SECRET", "x"), ("CLAUDE_CONFIG_DIR", "/c")]));
        assert_eq!(shared, vars(&[("PATH", "/bin"), ("CLAUDE_CONFIG_DIR", "/c")]));
        assert!(shared_environment(Vec::new()).is_empty());
    }

    #[test]
    fn test_c_d7_windows_path_key_is_kept_as_is() {
        assert_eq!(
            shared_environment(vars(&[("Path", "C:\\a"), ("PATHEXT", ".EXE")])),
            vars(&[("Path", "C:\\a"), ("PATHEXT", ".EXE")])
        );
    }
}
