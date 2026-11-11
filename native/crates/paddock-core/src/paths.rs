//! 경로 글자 규칙: 폴더·파일 경로의 철자 맞춤, 같은 폴더 판정, 안팎 판정, 이름 조각 나누기·잇기.
//! 화면과 파일 시스템을 모르는 순수 계산이다.
//!
//! 경로를 비교하거나 열쇠로 쓰는 곳은 직접 글자를 자르지 않고 이 모듈을 거친다 — 윈도 드라이브·WSL 공유·리눅스 경로의
//! 철자와 대소문자 규칙이 이 한 곳에 있다.

/// 경로가 Windows 모양(드라이브 글자 또는 `\\서버\공유`)인지.
fn is_windows_style(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("\\\\") || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// 작업 폴더의 key. 폴더를 가리키는 경로를 끝 구분자만 다른 같은 폴더가 한 줄이 되도록 맞춘 글자다.
///
/// 드라이브 경로는 구분자를 `/`로, 드라이브 글자를 대문자로 맞춘다(OSC 7과 탐색기 경로가 섞여 와도 같은 폴더가 되게).
/// `\\wsl.localhost\…` 같은 공유 경로는 역슬래시를 그대로 둔다 — `wsl.exe`가 이 모양으로 받는다.
/// 루트(`/`·`C:/`)의 구분자는 남긴다.
///
/// Examples
/// --------
/// | 경로                            | key                           |
/// |---------------------------------|-------------------------------|
/// | `/home/me/`                     | `/home/me`                    |
/// | `c:\Users\me\`                  | `C:/Users/me`                 |
/// | `\\wsl.localhost\Ubuntu\home\`  | `\\wsl.localhost\Ubuntu\home` |
/// | `/`                             | `/`                           |
pub fn folder_key(path: &str) -> String {
    let trimmed = path.trim();
    let bytes = trimmed.as_bytes();
    let mut key = trimmed.to_string();
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        // 같은 공유 경로가 OSC 7에서는 슬래시로, WSL 조사에서는 역슬래시로 온다. 역슬래시 한 가지 철자로 모은다.
        key = trimmed.replace('/', "\\");
        if let Some(host_end) = key[2..].find('\\').map(|at| at + 2) {
            let host = key[2..host_end].to_ascii_lowercase();
            if host == "wsl.localhost" || host == "wsl$" {
                key.replace_range(2..host_end, &host);
            }
        }
        while key.len() > 2 && (key.ends_with('\\') || key.ends_with('/')) {
            key.pop();
        }
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        key = trimmed.replace('\\', "/");
        key.replace_range(..1, &trimmed[..1].to_ascii_uppercase());
        while key.len() > 3 && key.ends_with('/') {
            key.pop();
        }
        if key.len() == 2 {
            key.push('/');
        }
    } else {
        while key.len() > 1 && (key.ends_with('/') || key.ends_with('\\')) {
            key.pop();
        }
    }
    key
}

/// 사용자에게 보이는 폴더 경로: 홈 아래는 `~`로 줄이고, WSL 공유(`\\wsl.localhost\\<배포판>\\…`)는 배포판 앞부분을 떼어 리눅스 경로로 보인다.
///
/// `home`은 이 컴퓨터 사용자의 홈이다. WSL 공유의 홈은 따로 묻지 않고 경로 모양(`/home/<사용자>`·`/root`)으로 알아본다.
/// 홈은 폴더 경계로만 맞는다(`/home/me2`는 `/home/me` 아래가 아니다). 그 밖의 경로는 key 그대로다.
///
/// Examples
/// --------
/// | key                                   | home        | 결과         |
/// |---------------------------------------|-------------|--------------|
/// | `/home/me/app`                        | `/home/me`  | `~/app`      |
/// | `\\wsl.localhost\\Ubuntu\\home\\me\\app` | (없음)      | `~/app`      |
/// | `\\wsl.localhost\\Ubuntu\\srv\\data`       | (없음)      | `/srv/data`  |
/// | `C:/Users/me`                         | `C:/Users/me` | `~`        |
/// | `/srv/data`                           | `/home/me`  | `/srv/data`  |
pub fn display_path(
    key: &str,
    home: Option<&str>,
) -> String {
    if let Some((_, _, linux)) = crate::git_paths::parse_wsl_path(key) {
        let parts: Vec<&str> = linux.split('/').filter(|part| !part.is_empty()).collect();
        let home_len = match parts.as_slice() {
            ["home", user, ..] => Some("/home/".len() + user.len()),
            ["root", ..] => Some("/root".len()),
            _ => None,
        };
        match home_len {
            Some(len) => format!("~{}", &linux[len..]),
            None => linux,
        }
    } else if let Some(home) = home.filter(|home| is_inside(home, key)) {
        let (key, home) = (folder_key(key), folder_key(home));
        format!("~{}", key[home.trim_end_matches(['/', '\\']).len()..].replace('\\', "/"))
    } else {
        key.to_string()
    }
}

/// 두 폴더 key를 같은 폴더인지 가르는 비교용 글자. 폴더를 "같다"고 보는 기준이 이 한 곳에 있다.
///
/// 드라이브 경로(`C:/…`)와 일반 공유 경로(`\\서버\\…`)는 윈도 파일 시스템이라 대소문자를 가리지 않으므로 모두 소문자로 맞춘다.
/// `\\wsl.localhost\\…`·`\\wsl$\\…`는 배포판 이름까지만 소문자로 맞추고 그 아래는 리눅스 파일 시스템이라 그대로 둔다.
/// 리눅스 경로도 그대로다. 표시는 이 글자가 아니라 원래 key로 한다.
///
/// Examples
/// --------
/// | key                           | 비교용 글자                  |
/// |-------------------------------|------------------------------|
/// | `C:/Users/Me`                 | `c:/users/me`                |
/// | `\\srv\\Share\\Dir`           | `\\srv\\share\\dir`          |
/// | `\\wsl.localhost\\Ubuntu\\App` | `\\wsl.localhost\\ubuntu\\App` |
/// | `/App`                        | `/App`                       |
pub fn folder_identity(key: &str) -> String {
    let key = folder_key(key);
    let is_wsl_share = key.starts_with("\\\\wsl.localhost\\") || key.starts_with("\\\\wsl$\\");
    if is_wsl_share {
        let distribution_end = key[2..].match_indices('\\').nth(1).map_or(key.len(), |(at, _)| at + 2);
        let (head, tail) = key.split_at(distribution_end);
        format!("{}{tail}", head.to_ascii_lowercase())
    } else if is_windows_style(&key) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

/// 두 경로가 같은 작업 폴더를 가리키는지. 끝 구분자와 윈도 경로의 대소문자 차이는 같은 폴더다.
pub fn same_folder(
    left: &str,
    right: &str,
) -> bool {
    folder_identity(left) == folder_identity(right)
}

/// `path`가 `key` 폴더 안(또는 그 폴더 자체)에 있는지.
pub fn is_inside(
    key: &str,
    path: &str,
) -> bool {
    let (key, path) = (folder_identity(key), folder_identity(path));
    let is_root = key.ends_with('/') || key.ends_with('\\');
    path == key
        || (is_root && path.starts_with(&key))
        || path.strip_prefix(&key).is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// 파일 하나를 가리키는 경로의 한 가지 철자(열쇠). 같은 파일이 `C:\x\a.js`·`C:/x/a.js`·`c:/x/./a.js`처럼 다른 철자로 와도 같은 열쇠가 되어
/// 탭이 둘로 열리지 않는다. 철자 규칙은 폴더와 같고(`folder_key`: 드라이브 글자 대문자·`/`, WSL 공유는 역슬래시·호스트 소문자),
/// 절대 경로의 `.`·`..` 조각은 글자로 정리한다. 상대 경로는 정리하지 않고 그대로 둔다.
///
/// Examples
/// --------
/// | 경로                                    | 열쇠                                  |
/// |-----------------------------------------|---------------------------------------|
/// | `c:\x\a.js`·`C:/x/./a.js`               | `C:/x/a.js`                           |
/// | `/w/shop/../x.md`                       | `/w/x.md`                             |
/// | `//WSL.localhost/Ubuntu/home/../a.js`   | `\\wsl.localhost\Ubuntu\a.js`         |
/// | `src/a.js`                              | `src/a.js`                            |
pub fn file_key(path: &str) -> String {
    let key = folder_key(path);
    let bytes = key.as_bytes();
    // (뿌리 글자, 구분자, 뿌리 뒤 나머지). 뿌리를 알 수 없는 경로(상대 경로)는 None이다.
    let split: Option<(String, char, &str)> = if let Some(unc) = key.strip_prefix("\\\\") {
        // 공유 경로의 앞 두 조각(호스트·공유 이름)은 정리 대상이 아니다.
        let mut parts = unc.splitn(3, '\\');
        match (parts.next(), parts.next()) {
            (Some(host), Some(share)) => Some((format!("\\\\{host}\\{share}"), '\\', parts.next().unwrap_or(""))),
            _ => None,
        }
    } else if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        Some((key[..2].to_string(), '/', &key[2..]))
    } else {
        key.strip_prefix('/').map(|rest| (String::new(), '/', rest))
    };
    match split {
        Some((root, separator, rest)) => {
            let mut parts: Vec<&str> = Vec::new();
            for part in rest.split(['/', '\\']) {
                match part {
                    "" | "." => {}
                    ".." => {
                        parts.pop();
                    }
                    part => parts.push(part),
                }
            }
            format!("{root}{separator}{}", parts.join(&separator.to_string()))
        }
        None => key,
    }
}

/// `parent` 폴더 안 `name` 항목의 경로. 부모가 쓰는 구분자(역슬래시가 있으면 `\`, 아니면 `/`)를 그대로 쓴다.
pub fn child_path(
    parent: &str,
    name: &str,
) -> String {
    let separator = if parent.contains('\\') { '\\' } else { '/' };
    if parent.ends_with(['/', '\\']) { format!("{parent}{name}") } else { format!("{parent}{separator}{name}") }
}

/// 경로의 파일 이름(마지막 구성 요소).
pub fn file_name_of(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// `path`의 부모 폴더 글자. 마지막 구분자(`/`·`\`) 앞까지이되 뿌리는 잘라 내지 않는다. 철자는 고치지 않는다.
///
/// 파일이 어느 작업 폴더에도 없을 때 "문서가 있는 폴더"를 구하거나, 이름 바꾸기가 같은 폴더 경로를 만들 때 쓴다.
/// 결과가 파일 시스템 경로로 쓰이므로 뿌리를 지킨다: 드라이브 바로 아래 항목의 부모는 `C:\`, `/` 바로 아래 항목의 부모는 `/`,
/// 공유 경로 `\\호스트\공유` 바로 아래 항목의 부모는 그 공유 자신이다. 공유 뿌리·호스트처럼 부모가 없는 경로와 구분자가 없는 경로는 빈 글자다.
///
/// Examples
/// --------
/// | 경로                           | 결과                       |
/// |--------------------------------|----------------------------|
/// | `/p/docs/a.md`                 | `/p/docs`                  |
/// | `C:\p\a.md`                    | `C:\p`                     |
/// | `C:\a.md`                      | `C:\`                     |
/// | `/a.md`                        | `/`                        |
/// | `\\wsl.localhost\Ubuntu\a`     | `\\wsl.localhost\Ubuntu`   |
/// | `\\wsl.localhost\Ubuntu`      | (빈 글자)                  |
/// | `\\wsl.localhost`              | (빈 글자)                  |
/// | `a.md`                         | (빈 글자)                  |
pub fn parent_of(path: &str) -> String {
    let bytes = path.as_bytes();
    match path.rfind(['/', '\\']) {
        None => String::new(),
        Some(at) => {
            let is_share = path.starts_with("\\\\") || path.starts_with("//");
            if is_share {
                // `\\호스트\공유`까지가 공유 뿌리다. 마지막 구분자가 그 안에 있으면 부모가 없다.
                let host_end = path[2..].find(['/', '\\']).map(|found| found + 2);
                let share_end = host_end
                    .and_then(|host_end| path[host_end + 1..].find(['/', '\\']).map(|found| found + host_end + 1))
                    .unwrap_or(path.len());
                if at < share_end { String::new() } else { path[..at].to_string() }
            } else if at == 0 || (at == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()) {
                path[..=at].to_string()
            } else {
                path[..at].to_string()
            }
        }
    }
}

/// `from` 폴더(또는 파일) 안에 있던 `path`가 `from`이 `to`로 옮겨진 뒤 갖는 경로. `path`가 `from` 자신이면 `to`이고, `from` 밖이면 None이다.
/// `to`의 구분자 철자를 따르고, 안팎 판정은 `is_inside`와 같다(윈도 경로의 대소문자 차이는 같은 경로로 본다).
///
/// Examples
/// --------
/// | from      | to        | path             | 결과          |
/// |-----------|-----------|------------------|---------------|
/// | `/p/src`  | `/p/lib`  | `/p/src`         | `/p/lib`      |
/// | `/p/src`  | `/p/lib`  | `/p/src/a/b.js`  | `/p/lib/a/b.js` |
/// | `C:/p/src`| `C:/p/lib`| `c:\p\Src\a.js`| `C:/p/lib/a.js` |
/// | `/p/src`  | `/p/lib`  | `/p/srcx/a.js`   | None          |
pub fn rebased(
    from: &str,
    to: &str,
    path: &str,
) -> Option<String> {
    relative_to(from, path).map(|relative| {
        if relative.is_empty() {
            to.to_string()
        } else {
            let separator = if to.contains('\\') { "\\" } else { "/" };
            child_path(to, &relative.replace('/', separator))
        }
    })
}

/// `path`의 마지막 구성 요소를 `name`으로 바꾼 경로. 앞부분(뿌리·구분자 철자)은 그대로 두고, 구분자가 없는 경로는 `name`만 남는다.
///
/// 이름 바꾸기·새 항목처럼 같은 폴더 안의 다른 이름 경로를 만드는 곳이 쓴다. `name`은 검사하지 않는다(이름 규칙은 `entry_name`이 가른다).
///
/// Examples
/// --------
/// | 경로                        | 새 이름   | 결과                        |
/// |-----------------------------|-----------|-----------------------------|
/// | `/p/src/a.js`               | `b.js`    | `/p/src/b.js`               |
/// | `C:\p\a.js`                 | `b.js`    | `C:\p\b.js`                 |
/// | `\\wsl.localhost\U\home\a`  | `b`       | `\\wsl.localhost\U\home\b`  |
/// | `/a.js`                     | `b.js`    | `/b.js`                     |
/// | `a.js`                      | `b.js`    | `b.js`                      |
pub fn with_file_name(
    path: &str,
    name: &str,
) -> String {
    match path.rfind(['/', '\\']) {
        Some(at) => format!("{}{name}", &path[..=at]),
        None => name.to_string(),
    }
}

/// `path`를 `root` 폴더 기준 상대 경로로 쓴다. 구분자는 `/`로 모으고, `root` 밖이면 None이며 `root` 자신이면 빈 글자다.
///
/// 안팎 판정은 `is_inside`와 같아서 윈도 경로의 대소문자 차이와 끝 구분자는 무시하고, 결과의 글자는 `path` 쪽 철자를 따른다.
/// `path`는 `file_key`로 정리된 경로여야 한다(`.`·`..` 조각을 풀지 않는다). 그래서 `..` 조각이 든 경로는 밖으로 나갈 수 있으므로 None이고,
/// 빈 `root`도 모든 경로를 품는 것처럼 보이지 않게 None이다. 목록·결과에 작업 폴더 기준 짧은 경로를 보일 때 쓴다.
///
/// Examples
/// --------
/// | root          | path                  | 결과                |
/// |---------------|-----------------------|---------------------|
/// | `/p`          | `/p/src/a.js`         | `src/a.js`          |
/// | `/p/`         | `/p/a.js`             | `a.js`              |
/// | `C:/Work`     | `c:\work\Src\a.js`    | `Src/a.js`          |
/// | `/p`          | `/p`                  | (빈 글자)           |
/// | `/p`          | `/pp/a.js`            | None                |
/// | `/`           | `/etc/hosts`          | `etc/hosts`         |
/// | `/p`          | `/p/../q/a.js`        | None                |
/// | (빈 글자)     | `/p/a.js`             | None                |
pub fn relative_to(
    root: &str,
    path: &str,
) -> Option<String> {
    let has_parent_step = path.split(['/', '\\']).any(|part| part == "..");
    if !root.trim().is_empty() && !has_parent_step && is_inside(root, path) {
        // 비교용 글자(`folder_identity`)는 `folder_key`와 길이가 같으므로, root 길이만큼 건너뛴 나머지가 상대 부분이다.
        let key = folder_key(path);
        let skipped = folder_identity(root).len();
        Some(key[skipped..].trim_start_matches(['/', '\\']).replace('\\', "/"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_folder_key_normalizes_separators_drive_letters_and_trailing_separators() {
        assert_eq!(folder_key("/home/me/"), "/home/me");
        assert_eq!(folder_key("/"), "/");
        assert_eq!(folder_key("c:\\Users\\me\\"), "C:/Users/me");
        assert_eq!(folder_key("C:\\"), "C:/");
        assert_eq!(folder_key("C:/"), "C:/");
        // WSL 공유 경로는 역슬래시를 그대로 둔다(wsl.exe가 이 모양으로 받는다).
        assert_eq!(folder_key("\\\\wsl.localhost\\Ubuntu\\home\\me\\"), "\\\\wsl.localhost\\Ubuntu\\home\\me");
        assert_eq!(folder_key("  /tmp/x  "), "/tmp/x");
    }

    #[test]
    fn test_c_folder_same_folder_rule_is_one_function_ignoring_case_only_for_windows_drive_and_share_paths() {
        assert!(same_folder("c:\\Users\\me", "C:\\users\\me\\"));
        assert!(same_folder("\\\\server\\Share\\Dir", "\\\\SERVER\\share\\dir"));
        // 리눅스 파일 시스템은 대소문자를 가린다: 리눅스 경로와 WSL 공유 경로의 배포판 이하.
        assert!(!same_folder("/App", "/app"));
        assert!(same_folder("\\\\wsl.localhost\\Ubuntu\\home\\me", "//WSL.localhost/ubuntu/home/me/"));
        assert!(!same_folder("\\\\wsl.localhost\\Ubuntu\\home\\App", "\\\\wsl.localhost\\Ubuntu\\home\\app"));
    }

    #[test]
    fn test_every_spelling_of_one_file_has_one_key() {
        let same_drive = ["C:\\x\\a.js", "C:/x/a.js", "c:/x/./a.js", "c:\\x\\y\\..\\a.js"];
        for spelling in same_drive {
            assert_eq!(file_key(spelling), "C:/x/a.js", "{spelling}");
        }
        assert_eq!(file_key("/w/shop/../x.md"), "/w/x.md");
        assert_eq!(file_key("/../a"), "/a");
        for spelling in ["//WSL.localhost/Ubuntu/home/../a.js", "\\\\wsl.localhost\\Ubuntu\\x\\..\\a.js"] {
            assert_eq!(file_key(spelling), "\\\\wsl.localhost\\Ubuntu\\a.js", "{spelling}");
        }
        // 상대 경로는 그대로이고, 공유 경로의 호스트·공유 이름은 `..`로 지워지지 않는다.
        assert_eq!(file_key("src/../a.js"), "src/../a.js");
        assert_eq!(file_key("//host/share/../../a"), "\\\\host\\share\\a");
    }

    #[test]
    fn test_c_files_13_child_paths_keep_the_separator_style_of_the_parent() {
        assert_eq!(child_path("/p/src", "a.js"), "/p/src/a.js");
        assert_eq!(child_path("C:/Users/me", "a.js"), "C:/Users/me/a.js");
        assert_eq!(child_path(r"\\wsl.localhost\Ubuntu\home", "a.js"), r"\\wsl.localhost\Ubuntu\home\a.js");
        assert_eq!(child_path("/", "etc"), "/etc");
        assert_eq!(child_path(r"C:\", "x"), r"C:\x");
    }

    #[test]
    fn test_parent_of_cuts_at_the_last_separator_of_either_style() {
        assert_eq!(parent_of("/p/docs/a.md"), "/p/docs");
        assert_eq!(parent_of("C:\\p\\a.md"), "C:\\p");
        assert_eq!(parent_of("C:/p\\mixed/a.md"), "C:/p\\mixed");
        assert_eq!(parent_of("a.md"), "");
        assert_eq!(parent_of(""), "");
        // 뿌리는 잘라 내지 않는다: 이 값이 그대로 파일 시스템 경로로 쓰인다.
        assert_eq!(parent_of("/a.md"), "/");
        assert_eq!(parent_of("C:\\a.md"), "C:\\");
        assert_eq!(parent_of("C:/a.md"), "C:/");
        assert_eq!(parent_of("\\\\wsl.localhost\\Ubuntu\\a"), "\\\\wsl.localhost\\Ubuntu");
        assert_eq!(parent_of("//wsl.localhost/Ubuntu/a/b"), "//wsl.localhost/Ubuntu/a");
        assert_eq!(parent_of("\\\\wsl.localhost\\Ubuntu"), "");
        assert_eq!(parent_of("\\\\wsl.localhost"), "");
    }

    #[test]
    fn test_with_file_name_keeps_everything_before_the_last_separator() {
        let cases = [
            ("/p/src/a.js", "b.js", "/p/src/b.js"),
            ("C:\\p\\a.js", "b.js", "C:\\p\\b.js"),
            ("\\\\wsl.localhost\\U\\home\\a", "b", "\\\\wsl.localhost\\U\\home\\b"),
            ("/a.js", "b.js", "/b.js"),
            ("a.js", "b.js", "b.js"),
            ("/p/src/", "x", "/p/src/x"),
        ];
        for (path, name, expected) in cases {
            assert_eq!(with_file_name(path, name), expected, "{path} -> {name}");
        }
    }

    #[test]
    fn test_relative_to_is_none_outside_the_root_and_uses_slashes_inside() {
        let cases = [
            ("/p", "/p/src/a.js", Some("src/a.js")),
            ("/p/", "/p/a.js", Some("a.js")),
            ("C:/Work", "c:\\work\\Src\\a.js", Some("Src/a.js")),
            ("C:\\", "C:/x/y", Some("x/y")),
            ("\\\\wsl.localhost\\Ubuntu\\home", "//WSL.localhost/ubuntu/home/me/a", Some("me/a")),
            ("/p", "/p", Some("")),
            ("/p", "/pp/a.js", None),
            ("/p/src", "/p/a.js", None),
            ("/App", "/app/a", None),
            ("/", "/etc/hosts", Some("etc/hosts")),
            ("/p", "/p/../q/a.js", None),
            ("/p", "/p/src/../a.js", None),
            ("", "/p/a.js", None),
        ];
        for (root, path, expected) in cases {
            assert_eq!(relative_to(root, path).as_deref(), expected, "{root} + {path}");
        }
    }

    #[test]
    fn test_rebased_moves_a_path_from_inside_the_old_folder_to_the_new_one() {
        let cases = [
            ("/p/src", "/p/lib", "/p/src", Some("/p/lib")),
            ("/p/src", "/p/lib", "/p/src/a/b.js", Some("/p/lib/a/b.js")),
            ("C:/p/src", "C:/p/lib", "c:\\p\\Src\\a.js", Some("C:/p/lib/a.js")),
            (
                "\\\\wsl.localhost\\U\\a",
                "\\\\wsl.localhost\\U\\b",
                "\\\\wsl.localhost\\U\\a\\x\\y",
                Some("\\\\wsl.localhost\\U\\b\\x\\y"),
            ),
            ("/p/src", "/p/lib", "/p/srcx/a.js", None),
            ("/p/src", "/p/lib", "/q/src/a.js", None),
        ];
        for (from, to, path, expected) in cases {
            assert_eq!(rebased(from, to, path).as_deref(), expected, "{from} -> {to}: {path}");
        }
    }

    #[test]
    fn test_display_path_shortens_the_home_and_shows_a_wsl_share_as_a_linux_path() {
        let cases = [
            // Electron `displayPath`의 사례.
            ("/home/me/app", Some("/home/me"), "~/app"),
            ("\\\\wsl.localhost\\Ubuntu\\home\\me\\app", None, "~/app"),
            ("\\\\wsl.localhost\\Ubuntu\\srv\\data", None, "/srv/data"),
            // 홈 자체, 배포판 루트, `wsl$` 표기, root 사용자의 홈.
            ("/home/me", Some("/home/me"), "~"),
            ("\\\\wsl.localhost\\Ubuntu\\home\\me", None, "~"),
            ("\\\\wsl.localhost\\Ubuntu", None, "/"),
            ("\\\\wsl$\\Debian\\root\\x", None, "~/x"),
            // 윈도 드라이브 홈은 대소문자·구분자가 달라도 홈이다. 홈 밖이나 이름만 닮은 폴더는 그대로다.
            ("C:/Users/me/app", Some("C:\\users\\ME"), "~/app"),
            ("C:/Users/me", Some("C:/Users/me"), "~"),
            ("C:/Users/me2/app", Some("C:/Users/me"), "C:/Users/me2/app"),
            ("/srv/data", Some("/home/me"), "/srv/data"),
            ("/home/me2", Some("/home/me"), "/home/me2"),
            ("/home/me", None, "/home/me"),
        ];
        for (key, home, expected) in cases {
            assert_eq!(display_path(key, home), expected, "{key} (home {home:?})");
        }
    }
}
