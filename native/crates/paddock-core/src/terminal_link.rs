//! 터미널 글자 위의 파일 경로 링크: 한 줄에서 경로꼴 글자를 찾고(`find_paths`), 터미널의 현재 폴더 기준으로 실제 경로를 만든다(`resolve`).
//!
//! 링크는 경로꼴 후보일 뿐이고, 실제 파일인지는 부르는 쪽이 파일 시스템에서 확인한다. 이 모듈은 파일 시스템을 모른다.

/// 줄 안의 경로 링크 하나. 위치는 글자(char) 단위다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpan {
    /// 링크 글자가 시작하는 글자 번호(0부터).
    pub start: usize,
    /// 링크 글자의 길이. 뒤에 붙은 `:줄:칸`도 포함한다.
    pub len: usize,
    /// `:줄:칸`을 뗀 경로 글자.
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// 글자가 경로 표시(`.`·`..`·`~`)뿐인지. 뒤에 줄·칸 번호(`:12`)가 붙어도 같다.
///
/// Examples
/// --------
/// | 글자             | 결과    |
/// |------------------|---------|
/// | `.`·`..`·`~`     | true    |
/// | `.:12`           | true    |
/// | `./src`          | false   |
/// | `~/notes.md`     | false   |
/// | `orders.js`      | false   |
pub fn is_bare_path_mark(text: &str) -> bool {
    let (mark, numbers) = text.split_once(':').unwrap_or((text, ""));
    matches!(mark, "." | ".." | "~") && numbers.split(':').all(|part| part.chars().all(|c| c.is_ascii_digit()))
}

/// 한 줄에서 경로꼴 글자를 모두 찾는다. 공백과 따옴표(`"`·`'`·`` ` ``)로 글자를 끊고, 앞의 여는 괄호와 뒤의 문장부호를 떼고,
/// 끝의 `:줄:칸` 번호를 경로에서 분리한다. 경로꼴은 구분자(`/`·`\`)가 있거나 확장자가 있거나 `~`로 시작하는 글자다.
/// 경로 표시뿐인 글자와 주소(`://`)는 링크가 아니다.
///
/// Examples
/// --------
/// | 줄                              | 링크 글자(경로, 줄, 칸)          |
/// |---------------------------------|----------------------------------|
/// | `echo src/a.js:12`              | `src/a.js:12` (`src/a.js`, 12)   |
/// | `at (x/b.ts:3:9),`              | `x/b.ts:3:9` (`x/b.ts`, 3, 9)    |
/// | `Fixed it. See src/orders.js.`  | `src/orders.js`                  |
/// | `echo ok ... done`              | (없음)                           |
pub fn find_paths(line: &str) -> Vec<LinkSpan> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if is_word_break(chars[index]) {
            index += 1;
        } else {
            let begin = index;
            while index < chars.len() && !is_word_break(chars[index]) {
                index += 1;
            }
            spans.extend(link_in(&chars, begin, index));
        }
    }
    spans
}

/// 링크의 경로를 실제 경로로 만든다. 상대 경로는 터미널의 현재 폴더 기준이고 `~`는 홈 폴더다.
/// 터미널이 WSL 폴더에 있으면 `/`로 시작하는 경로를 그 배포판의 공유 경로(`\\wsl.localhost\배포판`)로 옮긴다.
/// 기준 폴더나 홈을 몰라 만들 수 없거나, WSL이 아닌 공유 주소(`\\서버\공유`)이면 None이다.
///
/// Examples
/// --------
/// | 경로                  | 현재 폴더                          | 홈          | 결과                                      |
/// |-----------------------|------------------------------------|-------------|-------------------------------------------|
/// | `src/a.js`            | `/w/shop`                          | -           | `/w/shop/src/a.js`                        |
/// | `./src/a.js`          | `C:\w\shop`                        | -           | `C:\w\shop\src\a.js`                      |
/// | `/etc/hosts`          | `/w/shop`                          | -           | `/etc/hosts`                              |
/// | `~/notes.md`          | `/w/shop`                          | `/home/me`  | `/home/me/notes.md`                       |
/// | `/home/me/a.js`       | `\\wsl.localhost\Ubuntu\home\me`   | -           | `\\wsl.localhost\Ubuntu\home\me\a.js`     |
/// | `~/a.md`              | `\\wsl.localhost\Ubuntu\home\me\p`  | -           | `\\wsl.localhost\Ubuntu\home\me\a.md`   |
/// | `\\server\share\a.js` | (아무거나)                         | -           | None                                      |
/// | `src/a.js`            | (모름)                             | -           | None                                      |
pub fn resolve(
    path: &str,
    terminal_cwd: Option<&str>,
    home: Option<&str>,
) -> Option<String> {
    let wsl_root = terminal_cwd.and_then(wsl_share_root);
    let cwd = terminal_cwd.filter(|cwd| !cwd.is_empty());
    let resolved = if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        match (wsl_root, cwd, home) {
            // WSL 폴더의 터미널에서 `~`는 리눅스 홈이다. 현재 폴더가 `…\home\<사용자>\…`이나 `…\root\…` 안일 때만 그 홈을 안다.
            (Some(root), Some(cwd), _) => {
                wsl_home_of(cwd, &root).map(|wsl_home| join(&wsl_home, path.get(2..).unwrap_or("")))
            }
            (None, _, Some(home)) => Some(join(home, path.get(2..).unwrap_or(""))),
            _ => None,
        }
    } else if is_absolute(path) {
        match wsl_root {
            Some(root) if path.starts_with('/') && !path.starts_with("//") => {
                Some(format!("{root}{}", path.replace('/', "\\")))
            }
            _ => Some(path.to_string()),
        }
    } else {
        cwd.map(|cwd| join(cwd, path))
    };
    // 공유 주소(`\\서버\공유`)는 열기만 해도 네트워크 접속(자격 증명 전달)이 일어난다. 링크는 올려 보는 것만으로 파일 여부를 확인하므로
    // WSL 공유(`wsl.localhost`·`wsl$`)만 통과시키고 나머지 공유 주소는 링크가 아니다.
    resolved.filter(|path| !is_share_address(path) || wsl_share_root(&path.replace('/', "\\")).is_some())
}

/// `\\서버\…`·`//서버/…` 꼴의 공유 주소인지.
fn is_share_address(path: &str) -> bool {
    path.starts_with("\\\\") || path.starts_with("//")
}

/// WSL 공유(`root`가 `wsl_share_root`) 위의 현재 폴더에서 리눅스 홈을 찾는다: `…\home\<사용자>`나 `…\root`. 홈 밖이면 None이다.
fn wsl_home_of(
    cwd: &str,
    root: &str,
) -> Option<String> {
    let below = cwd.get(root.len()..)?;
    let parts: Vec<&str> = below.split('\\').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        ["home", user, ..] => Some(format!("{root}\\home\\{user}")),
        ["root", ..] => Some(format!("{root}\\root")),
        _ => None,
    }
}

/// 링크 글자를 끊는 글자: 공백과 따옴표.
fn is_word_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`')
}

/// 글자 `chars[begin..end]` 하나가 링크이면 그 링크를 돌려준다.
fn link_in(
    chars: &[char],
    begin: usize,
    end: usize,
) -> Option<LinkSpan> {
    let mut start = begin;
    let mut stop = end;
    while start < stop && matches!(chars[start], '(' | '[' | '{' | '<') {
        start += 1;
    }
    while stop > start && matches!(chars[stop - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>') {
        stop -= 1;
    }
    // 끝의 `:줄` 또는 `:줄:칸`을 뗀다. 경로가 비지 않게 `:` 앞에 글자가 남아야 한다.
    let mut path_end = stop;
    let mut numbers: Vec<u32> = Vec::new();
    while numbers.len() < 2 {
        let mut digits = path_end;
        while digits > start && chars[digits - 1].is_ascii_digit() {
            digits -= 1;
        }
        if digits < path_end && digits > start + 1 && chars[digits - 1] == ':' {
            numbers.push(chars[digits..path_end].iter().collect::<String>().parse().unwrap_or(0));
            path_end = digits - 1;
        } else {
            break;
        }
    }
    let path: String = chars[start..path_end].iter().collect();
    let is_link = !is_bare_path_mark(&path)
        && !path.contains("://")
        && path.chars().any(char::is_alphanumeric)
        && (path.contains(['/', '\\']) || has_extension(&path));
    let (line, column) = match numbers.as_slice() {
        [column, line] => (Some(*line), Some(*column)),
        [line] => (Some(*line), None),
        _ => (None, None),
    };
    is_link.then(|| LinkSpan { start, len: stop - start, path, line, column })
}

/// `이름.확장자` 모양인지(점 앞에 글자가 있고 점 뒤가 1~10자의 영숫자·`_`·`-`).
fn has_extension(path: &str) -> bool {
    match path.rfind('.') {
        Some(dot) if dot > 0 => {
            let tail = &path[dot + 1..];
            (1..=10).contains(&tail.chars().count())
                && tail.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        }
        _ => false,
    }
}

/// 경로가 현재 폴더와 무관한 절대 경로인지(`/…`·`\…`·`C:…`).
fn is_absolute(path: &str) -> bool {
    let mut chars = path.chars();
    let first = chars.next();
    path.starts_with(['/', '\\']) || (first.is_some_and(|c| c.is_ascii_alphabetic()) && chars.next() == Some(':'))
}

/// 터미널의 현재 폴더가 WSL 공유(`\\wsl.localhost\배포판\…`)이면 `\\wsl.localhost\배포판`을 돌려준다.
fn wsl_share_root(cwd: &str) -> Option<String> {
    let rest = cwd.strip_prefix("\\\\")?;
    let mut parts = rest.split('\\');
    let host = parts.next()?;
    let distro = parts.next().filter(|distro| !distro.is_empty())?;
    let is_wsl = host.eq_ignore_ascii_case("wsl.localhost") || host.eq_ignore_ascii_case("wsl$");
    is_wsl.then(|| format!("\\\\{host}\\{distro}"))
}

/// `base` 폴더 안 `rest` 경로. `.`은 건너뛰고 `..`은 한 단계 올라가며, 구분자는 `base`가 쓰는 것(역슬래시가 있으면 `\`)을 따른다.
fn join(
    base: &str,
    rest: &str,
) -> String {
    let separator = if base.contains('\\') { '\\' } else { '/' };
    let mut base = base.trim_end_matches(['/', '\\']).to_string();
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none()
                    && let Some(cut) = base.rfind(separator)
                {
                    base.truncate(cut);
                }
            }
            part => parts.push(part),
        }
    }
    let tail = parts.join(&separator.to_string());
    if tail.is_empty() { base } else { format!("{base}{separator}{tail}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        find_paths(line).iter().map(|span| chars[span.start..span.start + span.len].iter().collect()).collect()
    }

    #[test]
    fn test_tl01_a_lone_dot_double_dot_or_tilde_is_not_a_path_link() {
        for text in [".", "..", "~", ".:12"] {
            assert!(is_bare_path_mark(text), "{text}");
        }
        for text in ["./src", "~/notes.md", "orders.js", "src/orders.js:3"] {
            assert!(!is_bare_path_mark(text), "{text}");
        }
    }

    #[test]
    fn test_tl02_sentence_ending_dots_are_dropped_and_real_paths_stay() {
        assert_eq!(texts("\u{25cf} Fixed it. See src/orders.js"), ["src/orders.js"]);
        assert_eq!(texts("See src/orders.js."), ["src/orders.js"]);
        assert_eq!(texts("wait . .. ~ done"), Vec::<String>::new());
    }

    #[test]
    fn test_c_link_1_the_line_and_column_numbers_are_split_from_the_path() {
        let table = [
            ("echo src/a.js:12", "src/a.js", Some(12), None),
            ("src/a.js:12:5", "src/a.js", Some(12), Some(5)),
            ("at (x/b.ts:3:9),", "x/b.ts", Some(3), Some(9)),
            ("src/a.js:12:", "src/a.js", Some(12), None),
            ("src/a.js", "src/a.js", None, None),
            ("C:\\w\\a.js:7", "C:\\w\\a.js", Some(7), None),
        ];
        for (line, path, number, column) in table {
            let spans = find_paths(line);
            assert_eq!(spans.len(), 1, "{line}");
            assert_eq!((spans[0].path.as_str(), spans[0].line, spans[0].column), (path, number, column), "{line}");
        }
        // 위치는 글자 단위이고 번호까지 포함한다.
        let span = &find_paths("go \u{d55c}\u{ae00} src/a.js:12 now")[0];
        assert_eq!((span.start, span.len), (6, 11));
    }

    #[test]
    fn test_c_link_2_a_relative_path_is_joined_to_the_current_folder_of_the_terminal() {
        let table = [
            ("src/a.js", "/w/shop", "/w/shop/src/a.js"),
            ("./src/a.js", "/w/shop/", "/w/shop/src/a.js"),
            ("src/a.js", "C:\\w\\shop", "C:\\w\\shop\\src\\a.js"),
            ("./src/a.js", "C:\\w\\shop", "C:\\w\\shop\\src\\a.js"),
            ("../x.md", "/w/shop", "/w/x.md"),
            ("/etc/hosts", "/w/shop", "/etc/hosts"),
            ("C:\\x\\a.js", "/w/shop", "C:\\x\\a.js"),
        ];
        for (path, cwd, expected) in table {
            assert_eq!(resolve(path, Some(cwd), None).as_deref(), Some(expected), "{path} in {cwd}");
        }
        assert_eq!(resolve("src/a.js", None, None), None);
        // WSL 폴더의 터미널이 찍은 리눅스 절대 경로는 그 배포판의 공유 경로가 된다.
        assert_eq!(
            resolve("/home/me/a.js", Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\proj"), None).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\a.js")
        );
        assert_eq!(
            resolve("src/a.js", Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\proj"), None).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\proj\\src\\a.js")
        );
    }

    #[test]
    fn test_c_link_3_a_tilde_path_starts_at_the_home_folder() {
        assert_eq!(resolve("~/notes.md", Some("/w/shop"), Some("/home/me")).as_deref(), Some("/home/me/notes.md"));
        assert_eq!(resolve("~/a/b.md", None, Some("C:\\Users\\me")).as_deref(), Some("C:\\Users\\me\\a\\b.md"));
        assert_eq!(resolve("~/notes.md", Some("/w/shop"), None), None);
        // WSL 폴더의 터미널에서 `~`는 현재 폴더의 리눅스 홈이고(윈도 홈이 아니다), 홈 밖(`/tmp`)에서는 알 수 없다.
        assert_eq!(
            resolve("~/n.md", Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\p"), Some("C:\\Users\\me")).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\n.md")
        );
        assert_eq!(
            resolve("~/n.md", Some("\\\\wsl.localhost\\Ubuntu\\root"), None).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\root\\n.md")
        );
        assert_eq!(resolve("~/n.md", Some("\\\\wsl.localhost\\Ubuntu\\tmp\\x"), Some("C:\\Users\\me")), None);
    }

    #[test]
    fn test_c_link_4_words_that_are_not_paths_are_never_links() {
        for line in [
            "echo hello world",
            "Fixed it. Done!",
            "12:30 pm",
            "see https://example.com/a.js now",
            "a / b",
            "...",
            "(wait)",
        ] {
            assert_eq!(texts(line), Vec::<String>::new(), "{line}");
        }
        assert_eq!(texts("a.js, b/c"), ["a.js", "b/c"]);
        assert_eq!(texts("\"my/path.txt\" and `lib/x.rs:3`"), ["my/path.txt", "lib/x.rs:3"]);
    }

    #[test]
    fn test_c_link_5_a_share_address_is_not_a_link_but_wsl_shares_are() {
        let table = [
            ("\\\\server\\share\\a.js", Some("C:\\w")),
            ("//server/share/a.js", Some("/w")),
            ("\\\\server\\share\\a.js", None),
            ("src/a.js", Some("\\\\server\\share\\proj")),
        ];
        for (path, cwd) in table {
            assert_eq!(resolve(path, cwd, None), None, "{path} in {cwd:?}");
        }
        assert_eq!(
            resolve("\\\\WSL.localhost\\Ubuntu\\home\\a.js", Some("C:\\w"), None).as_deref(),
            Some("\\\\WSL.localhost\\Ubuntu\\home\\a.js")
        );
        assert_eq!(resolve("//wsl$/Ubuntu/home/a.js", Some("C:\\w"), None).as_deref(), Some("//wsl$/Ubuntu/home/a.js"));
    }
}
