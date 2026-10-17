//! 터미널 환경의 PATH 맨 앞에 Claude 실행 래퍼 폴더를 붙이는 순수 규칙.
//!
//! 래퍼 폴더(`~/.paddock/config/bin`)에는 기존 앱이 `claude` 래퍼를 놓는다. 래퍼는 일반 터미널에서 친 `claude`에도
//! Paddock 전용 설정(`--settings`)이 붙게 한다. 이 앱은 래퍼 파일을 만들지 않고, 폴더가 있을 때 PATH만 앞에 붙인다.

/// 환경에서 PATH 항목의 위치. Windows는 `Path`처럼 대소문자가 달라도 같은 변수다.
fn path_index(
    env: &[(String, String)],
    is_windows: bool,
) -> Option<usize> {
    env.iter().position(|(name, _)| if is_windows { name.eq_ignore_ascii_case("path") } else { name == "PATH" })
}

/// `env`의 PATH 맨 앞에 `wrapper_directory`를 붙인 새 환경을 돌려준다. 받은 환경은 바꾸지 않는다.
///
/// 기준 PATH는 `env`의 것이고, 없으면 `base_env`(보통 이 프로세스 환경)의 것이다. 이미 맨 앞에 있으면 다시 붙이지 않는다.
/// Windows의 `Path` 키는 이름을 유지하고 `;`로 잇는다. 어느 쪽에도 PATH가 없으면 래퍼 폴더 하나만 든 PATH가 생긴다.
///
/// Examples
/// --------
/// | env.PATH                | wrapper_directory | 결과 PATH            |
/// |-------------------------|-------------------|----------------------|
/// | `/usr/bin`              | `/p/bin`          | `/p/bin:/usr/bin`    |
/// | `/p/bin:/usr/bin`       | `/p/bin`          | `/p/bin:/usr/bin`    |
/// | (없음, base_env `/x`)   | `/p/bin`          | `/p/bin:/x`          |
/// | `C:\a;C:\b` (키 `Path`) | `C:\p\bin`        | `C:\p\bin;C:\a;C:\b` |
pub fn with_wrapper_path(
    env: &[(String, String)],
    wrapper_directory: &str,
    base_env: &[(String, String)],
    is_windows: bool,
) -> Vec<(String, String)> {
    let separator = if is_windows { ';' } else { ':' };
    let own = path_index(env, is_windows);
    let base = path_index(base_env, is_windows);
    let key = own
        .map(|index| env[index].0.clone())
        .or_else(|| base.map(|index| base_env[index].0.clone()))
        .unwrap_or_else(|| (if is_windows { "Path" } else { "PATH" }).to_string());
    let current = own.map(|index| env[index].1.as_str()).or_else(|| base.map(|index| base_env[index].1.as_str()));
    let current = current.unwrap_or("");
    let entries: Vec<&str> = current.split(separator).filter(|entry| !entry.is_empty()).collect();
    let next = if entries.first() == Some(&wrapper_directory) {
        current.to_string()
    } else {
        std::iter::once(wrapper_directory).chain(entries).collect::<Vec<_>>().join(&separator.to_string())
    };
    let mut result = env.to_vec();
    match own {
        Some(index) => result[index].1 = next,
        None => result.push((key, next)),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
    }

    fn value_of<'a>(
        env: &'a [(String, String)],
        name: &str,
    ) -> Option<&'a str> {
        env.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    #[test]
    fn test_c_wrapper_7_the_wrapper_folder_goes_first_once_and_the_input_environment_is_unchanged() {
        let own = env(&[("PATH", "/usr/bin:/bin"), ("HOME", "/h")]);
        let next = with_wrapper_path(&own, "/p/bin", &[], false);
        assert_eq!(value_of(&next, "PATH"), Some("/p/bin:/usr/bin:/bin"));
        assert_eq!(value_of(&next, "HOME"), Some("/h"));
        assert_eq!(value_of(&own, "PATH"), Some("/usr/bin:/bin"));
        assert_eq!(value_of(&with_wrapper_path(&next, "/p/bin", &[], false), "PATH"), Some("/p/bin:/usr/bin:/bin"));
        let from_base = with_wrapper_path(&[], "/p/bin", &env(&[("PATH", "/x")]), false);
        assert_eq!(value_of(&from_base, "PATH"), Some("/p/bin:/x"));
        assert_eq!(value_of(&with_wrapper_path(&[], "/p/bin", &[], false), "PATH"), Some("/p/bin"));
    }

    #[test]
    fn test_c_wrapper_8_windows_keeps_the_path_key_spelling_and_joins_with_semicolons() {
        let next = with_wrapper_path(&env(&[("Path", "C:\\a;C:\\b")]), "C:\\p\\bin", &[], true);
        assert_eq!(next, env(&[("Path", "C:\\p\\bin;C:\\a;C:\\b")]));
        let from_base = with_wrapper_path(&[], "C:\\p\\bin", &env(&[("Path", "C:\\x")]), true);
        assert_eq!(from_base, env(&[("Path", "C:\\p\\bin;C:\\x")]));
    }
}
