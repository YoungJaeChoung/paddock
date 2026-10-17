//! Markdown 파일 탭의 규칙: 보기 모드(원문·미리보기·나란히)와 파일별 기억, 본문 속 이미지 경로 풀이, 링크 열기 허용 범위.
//!
//! 화면에 그리는 일은 앱이 하고, 여기서는 글자와 경로만 받아 판정하는 순수 규칙만 둔다.
//! 미리보기가 네트워크에 닿지 않게 하는 규칙(원격 이미지 거부·링크 허용 범위)이 이 파일에 모여 있다. 원문 HTML(`<img src>`·`<a href>`)의
//! 주소도 렌더러가 같은 이미지 원천·링크 처리기로 넘기므로 이 규칙을 그대로 지난다.
use std::collections::HashMap;

/// Markdown 탭이 보여 주는 방식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// 원문(읽기 전용 텍스트 보기)만.
    Source,
    /// 그려 낸 미리보기만.
    Preview,
    /// 왼쪽 원문, 오른쪽 미리보기.
    Both,
}

impl View {
    /// 탭 줄 단추에 보이는 글자.
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "File",
            Self::Preview => "Preview",
            Self::Both => "Both",
        }
    }

    /// 상태 글과 자동 점검 명령에 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Self::Source => "file",
            Self::Preview => "preview",
            Self::Both => "both",
        }
    }

    /// `name`의 거꾸로. 모르는 이름은 None이다.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.name() == name)
    }

    /// 단추 순서(왼쪽부터).
    pub const ALL: [View; 3] = [View::Source, View::Preview, View::Both];
}

/// 파일마다 마지막으로 고른 보기 모드. 고르지 않은 파일은 미리보기다.
#[derive(Debug, Default)]
pub struct ViewChoices {
    chosen: HashMap<String, View>,
}

impl ViewChoices {
    pub fn get(
        &self,
        path: &str,
    ) -> View {
        self.chosen.get(path).copied().unwrap_or(View::Preview)
    }

    pub fn set(
        &mut self,
        path: &str,
        view: View,
    ) {
        self.chosen.insert(path.to_string(), view);
    }
}

/// 이미지로 읽을 수 있는 확장자(소문자). 본문 속 이미지 주소가 이 밖이면 파일을 읽지 않는다.
const IMAGE_EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "svg"];

/// 바깥 프로그램으로 열어도 되는 링크 머리(소문자). 파일·실행 프로토콜은 열지 않는다.
const LINK_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// 글자 앞이 `<scheme>:` 꼴인지(`C:`처럼 한 글자는 드라이브 이름이라 제외한다).
fn has_scheme(url: &str) -> bool {
    match url.find(':') {
        Some(at) if at >= 2 => url[..at].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => false,
    }
}

/// `%20` 같은 퍼센트 부호를 푼다. 잘못된 부호나 UTF-8이 아니게 되는 경우는 풀지 않은 그대로 둔다.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |byte: u8| (byte as char).to_digit(16);
        match (bytes[at], bytes.get(at + 1).copied().and_then(hex), bytes.get(at + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                decoded.push((high * 16 + low) as u8);
                at += 3;
            }
            (byte, _, _) => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_string())
}

/// 본문 속 이미지 주소를 디스크의 파일 경로로 푼다. 작업 폴더(`root_dir`) 안의 로컬 이미지 파일이 아니면 None이다.
///
/// 상대 주소는 문서가 있는 폴더(`base_dir`) 기준이고 `..`을 풀되 폴더 뿌리 위로는 올라가지 않는다. 절대 주소는
/// 드라이브 글자(`C:`)나 `/`로 시작하는 것만 받는다. 풀린 경로가 `root_dir`(문서가 속한 작업 폴더) 밖이면 모두 거부한다.
/// 원격 주소(`http`·`file`·`data` 등 머리가 있는 것)와 `//`·`\\`로 시작하는 네트워크 공유 주소, 질의(`?`)가 붙은 주소, 이미지 확장자가 아닌 파일은 모두 None이다 — 문서가 읽는 순간 어딘가에 연결하거나
/// 엉뚱한 파일을 읽게 만들 수 없다. 문서 자신이 `//host/share/…` 안에 있으면 그 공유 안의 상대 경로는 풀린다.
///
/// Examples
/// --------
/// | 문서 폴더          | 주소                      | 결과                     |
/// |--------------------|---------------------------|--------------------------|
/// | `C:/p/docs`        | `img/a.png`               | `C:/p/docs/img/a.png`    |
/// | `C:/p/docs`        | `../shot.jpg`             | `C:/p/shot.jpg`          |
/// | `C:/p/docs`        | `my%20pic.png`            | `C:/p/docs/my pic.png`   |
/// | `C:/p/docs`        | `https://e.com/a.png`     | None                     |
/// | `C:/p/docs`        | `\\server\share\a.png`   | None                     |
pub fn resolve_image_path(
    base_dir: &str,
    root_dir: &str,
    url: &str,
) -> Option<String> {
    let url = url.trim();
    let slashed = url.replace('\\', "/");
    let decoded = percent_decode(&slashed);
    let is_absolute = decoded.starts_with('/') || decoded.as_bytes().get(1) == Some(&b':');
    let extension = decoded.rsplit('.').next().unwrap_or("").to_lowercase();
    let acceptable = !decoded.is_empty()
        && !has_scheme(&decoded)
        && !decoded.starts_with("//")
        && !decoded.contains('?')
        && decoded.contains('.')
        && IMAGE_EXTENSIONS.contains(&extension.as_str());
    if acceptable {
        let combined = if is_absolute {
            decoded
        } else {
            format!("{}/{}", base_dir.replace('\\', "/").trim_end_matches('/'), decoded)
        };
        // 뿌리(`C:`·`//host/share`·`/`)와 그 아래 구성 요소를 나눠 `.`·`..`을 푼다.
        let (root, rest) = split_root(&combined);
        let mut parts: Vec<&str> = Vec::new();
        for part in rest.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        let joined = parts.join("/");
        // `..`으로 올라간 뒤의 이름도 이미지 확장자여야 한다.
        let is_image = IMAGE_EXTENSIONS.contains(&joined.rsplit('.').next().unwrap_or("").to_lowercase().as_str());
        let resolved = format!("{root}{joined}");
        if joined.is_empty() || !is_image || !is_within(&resolved, root_dir) { None } else { Some(resolved) }
    } else {
        None
    }
}

/// `path`가 `root_dir` 폴더 자신이거나 그 안인지. 드라이브·공유 경로(Windows 꼴)는 대소문자를 구별하지 않는다.
fn is_within(
    path: &str,
    root_dir: &str,
) -> bool {
    let normal = |text: &str| text.replace('\\', "/").trim_end_matches('/').to_string();
    let (path, root) = (normal(path), normal(root_dir));
    let windows_like = root.starts_with("//") || root.as_bytes().get(1) == Some(&b':');
    let (path, root) = if windows_like { (path.to_lowercase(), root.to_lowercase()) } else { (path, root) };
    root.is_empty() || path == root || path.starts_with(&format!("{root}/"))
}

/// 경로를 (뿌리, 나머지)로 나눈다. 뿌리는 `C:/`·`//host/share/`·`/` 중 하나이며 끝에 `/`를 포함한다.
fn split_root(path: &str) -> (String, &str) {
    if let Some(unc) = path.strip_prefix("//") {
        let mut pieces = unc.splitn(3, '/');
        let (host, share, rest) =
            (pieces.next().unwrap_or(""), pieces.next().unwrap_or(""), pieces.next().unwrap_or(""));
        (format!("//{host}/{share}/"), rest)
    } else if path.as_bytes().get(1) == Some(&b':') {
        (format!("{}/", &path[..2]), path[2..].trim_start_matches('/'))
    } else {
        ("/".to_string(), path.trim_start_matches('/'))
    }
}

/// 링크를 바깥 프로그램으로 열어도 되는 주소인지. 허용하는 것만 돌려주고 나머지는 None이다.
///
/// `http`·`https`·`mailto`만 받는다. 공백·제어 문자가 든 주소와 `-`로 시작하는 글자(명령 옵션으로 읽힐 수 있음)는 거부한다.
/// 파일 경로·`file:`·`javascript:`·기타 프로토콜 처리기는 열지 않는다.
///
/// Examples
/// --------
/// | 주소                         | 결과                  |
/// |------------------------------|-----------------------|
/// | `https://example.com/a?b=1`  | 그대로                |
/// | `mailto:a@b.com`             | 그대로                |
/// | `file:///C:/x.exe`           | None                  |
/// | `#section`                   | None                  |
pub fn external_link(url: &str) -> Option<String> {
    let lowered = url.to_lowercase();
    let allowed = LINK_SCHEMES.iter().any(|scheme| lowered.starts_with(scheme))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if allowed { Some(url.to_string()) } else { None }
}

/// 미리보기를 이루는 조각 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// 보통 본문(인용이 아닌 부분).
    Body(String),
    /// 인용 블록 하나. `>` 표지를 뗀 안쪽 글자다.
    Quote(String),
}

/// 원문을 최상위 인용 블록(`>`로 시작하는 줄 묶음)을 경계로 조각으로 나눈다.
///
/// 렌더러의 인용 블록은 왼쪽에만 선을 긋는데, 제품 화면은 한쪽 띠 대신 바탕색과 전체 테두리를 쓰므로 인용 부분을 따로 그린다.
/// 울타리 코드 블록(```` ``` ````·`~~~`) 안의 `>`는 인용이 아니다. 인용은 빈 줄을 만나거나, 인용 줄 뒤에 이어지는 줄이 다른 블록(제목·목록·코드 울타리)을
/// 시작하면 끝난다. 참조식 링크 정의(`[id]: 주소`)는 어느 조각에서든 쓸 수 있도록 모든 조각 끝에 붙인다. 인용이 하나도 없으면 원문 전체가 한 조각이다.
///
/// Examples
/// --------
/// | 원문                       | 결과                                         |
/// |----------------------------|----------------------------------------------|
/// | `a\n\n> q\n> r\n\nb`       | Body(`a`) Quote(`q\nr`) Body(`b`)           |
/// | `` ```\n> not\n``` ``      | Body 하나                                    |
pub fn split_blockquotes(text: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut body = String::new();
    let mut quote: Option<String> = None;
    let mut fence: Option<char> = None;
    let mut definitions: Vec<String> = Vec::new();
    let flush_body = |body: &mut String, segments: &mut Vec<Segment>| {
        if !body.trim().is_empty() {
            segments.push(Segment::Body(std::mem::take(body)));
        } else {
            body.clear();
        }
    };
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        let starts_other_block = marker.is_some()
            || trimmed.starts_with('#')
            || trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed
                .split_once(". ")
                .is_some_and(|(number, _)| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()));
        match (fence, marker) {
            (Some(open), Some(close)) if open == close => {
                fence = None;
                body.push_str(line);
            }
            (Some(_), _) => body.push_str(line),
            (None, Some(open)) => {
                if let Some(inner) = quote.take() {
                    segments.push(Segment::Quote(inner));
                }
                fence = Some(open);
                body.push_str(line);
            }
            (None, None) => {
                if is_definition(trimmed) && indent <= 3 {
                    definitions.push(line.trim_end_matches(['\r', '\n']).to_string());
                }
                if indent <= 3 && trimmed.starts_with('>') {
                    if quote.is_none() {
                        flush_body(&mut body, &mut segments);
                    }
                    let inner = trimmed[1..].strip_prefix(' ').unwrap_or(&trimmed[1..]);
                    quote.get_or_insert_with(String::new).push_str(inner);
                } else if quote.is_some() && !trimmed.trim().is_empty() && !starts_other_block {
                    // 인용 문단의 이어지는 줄(`>` 없이 이어 쓴 줄)도 인용에 속한다.
                    if let Some(inner) = quote.as_mut() {
                        inner.push_str(line);
                    }
                } else {
                    if let Some(inner) = quote.take() {
                        segments.push(Segment::Quote(inner));
                    }
                    body.push_str(line);
                }
            }
        }
    }
    if let Some(inner) = quote.take() {
        segments.push(Segment::Quote(inner));
    }
    flush_body(&mut body, &mut segments);
    if !definitions.is_empty() && segments.iter().any(|segment| matches!(segment, Segment::Quote(_))) {
        let tail = format!("\n\n{}\n", definitions.join("\n"));
        for segment in &mut segments {
            match segment {
                Segment::Body(text) | Segment::Quote(text) => text.push_str(&tail),
            }
        }
    }
    if segments.is_empty() {
        segments.push(Segment::Body(text.to_string()));
    }
    segments
}

/// `[id]: 주소` 꼴의 참조식 링크 정의 줄인지.
fn is_definition(trimmed: &str) -> bool {
    trimmed.strip_prefix('[').and_then(|rest| rest.split_once("]:")).is_some_and(|(id, target)| {
        !id.is_empty() && !id.contains(']') && !id.starts_with('^') && !target.trim().is_empty()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_md_1_a_file_never_chosen_opens_as_preview() {
        let choices = ViewChoices::default();
        assert_eq!(choices.get("/p/README.md"), View::Preview);
    }

    #[test]
    fn test_c_md_2_each_file_remembers_its_own_mode() {
        let mut choices = ViewChoices::default();
        choices.set("/p/a.md", View::Both);
        choices.set("/p/b.md", View::Source);
        assert_eq!(choices.get("/p/a.md"), View::Both);
        assert_eq!(choices.get("/p/b.md"), View::Source);
        assert_eq!(choices.get("/p/c.md"), View::Preview);
        choices.set("/p/a.md", View::Preview);
        assert_eq!(choices.get("/p/a.md"), View::Preview);
    }

    #[test]
    fn test_c_md_3_the_three_buttons_read_file_preview_both_and_names_round_trip() {
        let labels: Vec<_> = View::ALL.iter().map(|view| view.label()).collect();
        assert_eq!(labels, ["File", "Preview", "Both"]);
        for view in View::ALL {
            assert_eq!(View::from_name(view.name()), Some(view));
        }
        assert_eq!(View::from_name("split"), None);
    }

    #[test]
    fn test_c_md_4_image_addresses_resolve_against_the_markdown_folder_and_only_to_local_files() {
        let base = "C:/proj/docs";
        let resolve = |url: &str| resolve_image_path(base, "C:/", url);
        assert_eq!(resolve("img/a.png").as_deref(), Some("C:/proj/docs/img/a.png"));
        assert_eq!(resolve("./img/a.PNG").as_deref(), Some("C:/proj/docs/img/a.PNG"));
        assert_eq!(resolve("../shot.jpg").as_deref(), Some("C:/proj/shot.jpg"));
        assert_eq!(resolve("my%20pic.png").as_deref(), Some("C:/proj/docs/my pic.png"));
        assert_eq!(resolve("sub\\a.gif").as_deref(), Some("C:/proj/docs/sub/a.gif"));
        assert_eq!(resolve_image_path(base, "D:/", "D:\\pics\\a.webp").as_deref(), Some("D:/pics/a.webp"));
        assert_eq!(resolve_image_path("/home/u/docs", "/", "/etc/logo.svg").as_deref(), Some("/etc/logo.svg"));
        assert_eq!(resolve_image_path("/home/u/docs", "/", "a.png").as_deref(), Some("/home/u/docs/a.png"));
        // 네트워크 공유 폴더 안의 문서는 그 공유 안에서만 풀린다.
        assert_eq!(
            resolve_image_path("//wsl.localhost/Ubuntu/home/u", "//wsl.localhost/Ubuntu/home/u", "a.png").as_deref(),
            Some("//wsl.localhost/Ubuntu/home/u/a.png")
        );
    }

    #[test]
    fn test_c_md_4_remote_and_non_image_addresses_never_resolve_so_nothing_reaches_the_network() {
        for url in [
            "http://example.com/a.png",
            "https://example.com/a.png",
            "HTTPS://example.com/a.png",
            "//example.com/a.png",
            "\\\\server\\share\\a.png",
            "file:///C:/a.png",
            "data:image/png;base64,AAAA",
            "ftp://x/a.png",
            "javascript:alert(1)",
            "",
            "notes.txt",
            "a.png?raw=1",
            "../../..",
        ] {
            assert_eq!(resolve_image_path("C:/proj/docs", "C:/proj", url), None, "{url}");
        }
        // 문서가 공유 폴더 안에 있어도 문서 쪽이 가리킨 다른 공유로는 가지 못한다.
        assert_eq!(
            resolve_image_path("//wsl.localhost/Ubuntu/home/u", "//wsl.localhost/Ubuntu/home/u", "//evil/x.png"),
            None
        );
        assert_eq!(
            resolve_image_path("//wsl.localhost/Ubuntu/home/u", "//wsl.localhost/Ubuntu/home/u", "\\\\evil\\x.png"),
            None
        );
    }

    #[test]
    fn test_c_md_5_only_web_and_mail_links_may_open_in_another_program() {
        assert_eq!(external_link("https://example.com/a?b=1").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(external_link("http://example.com").as_deref(), Some("http://example.com"));
        assert_eq!(external_link("mailto:a@b.com").as_deref(), Some("mailto:a@b.com"));
        assert_eq!(external_link("HTTPS://Example.com").as_deref(), Some("HTTPS://Example.com"));
        for url in [
            "",
            "#section",
            "file:///C:/Windows/System32/calc.exe",
            "C:\\Windows\\System32\\calc.exe",
            "\\\\server\\share",
            "javascript:alert(1)",
            "ms-msdt:/id",
            "-flag",
            "https://a b.com",
            "https://a.com/\nx",
            "readme.md",
        ] {
            assert_eq!(external_link(url), None, "{url:?}");
        }
    }

    #[test]
    fn test_c_md_6_image_addresses_must_stay_inside_the_work_folder() {
        let inside = |url: &str| resolve_image_path("C:/proj/docs", "C:/proj", url);
        assert_eq!(inside("../shot.png").as_deref(), Some("C:/proj/shot.png"));
        assert_eq!(inside(r"c:\PROJ\img\a.png").as_deref(), Some("c:/PROJ/img/a.png"));
        // 작업 폴더 밖: `..`로 올라가거나 다른 폴더·드라이브를 가리키는 주소는 읽지 않는다.
        assert_eq!(inside("../../secret.png"), None);
        assert_eq!(inside("D:/other/a.png"), None);
        assert_eq!(inside("C:/proj-other/a.png"), None);
        assert_eq!(inside("C:/Users/me/.ssh/key.png"), None);
        assert_eq!(resolve_image_path("/home/u/p/docs", "/home/u/p", "../../x.png"), None);
        assert_eq!(resolve_image_path("/home/u/p/docs", "/home/u/p", "/etc/x.png"), None);
        assert_eq!(resolve_image_path("/home/u/p/docs", "/home/u/p", "../a.png").as_deref(), Some("/home/u/p/a.png"));
    }

    #[test]
    fn test_c_md_7_html_attribute_addresses_get_the_same_refusals_as_markdown_ones() {
        // 원문 HTML(`<img src>`·`<a href>`)의 주소도 이미지 원천·링크 처리기로 같은 규칙을 지난다.
        for src in [
            "http://localhost:1/x.png",
            "\\\\server\\x.png",
            "//server/x.png",
            "file:///C:/x.png",
            "data:image/png;base64,AA",
        ] {
            assert_eq!(resolve_image_path("C:/p", "C:/p", src), None, "{src}");
        }
        for href in
            ["file:C:/Windows/System32/calc.exe", "file:///C:/x", "\\\\server\\share", "javascript:void(0)", "#top"]
        {
            assert_eq!(external_link(href), None, "{href}");
        }
    }

    #[test]
    fn test_c_review_4_top_level_quotes_split_out_and_the_rest_stays_whole() {
        let parts = split_blockquotes("intro\n\n> q1\n> q2\n\nafter\n");
        assert_eq!(
            parts,
            [Segment::Body("intro\n\n".into()), Segment::Quote("q1\nq2\n".into()), Segment::Body("\nafter\n".into()),]
        );
        assert_eq!(split_blockquotes("no quotes\n"), [Segment::Body("no quotes\n".into())]);
        assert_eq!(split_blockquotes(""), [Segment::Body(String::new())]);
        assert_eq!(split_blockquotes("> only"), [Segment::Quote("only".into())]);
    }

    #[test]
    fn test_c_review_4_a_greater_than_inside_a_code_fence_is_not_a_quote_and_lazy_lines_stay_in_the_quote() {
        let fenced = "```\n> not a quote\n```\n";
        assert_eq!(split_blockquotes(fenced), [Segment::Body(fenced.into())]);
        let lazy = split_blockquotes("> first\nlazy continuation\n\nnext\n");
        assert_eq!(lazy[0], Segment::Quote("first\nlazy continuation\n".into()));
        let stops = split_blockquotes("> q\n# Heading\n");
        assert_eq!(stops, [Segment::Quote("q\n".into()), Segment::Body("# Heading\n".into())]);
        // 안쪽 인용 표지는 한 겹만 뗀다.
        assert_eq!(split_blockquotes("> > inner\n")[0], Segment::Quote("> inner\n".into()));
    }

    #[test]
    fn test_c_review_4_reference_link_definitions_are_copied_to_every_piece_so_links_keep_working() {
        let parts = split_blockquotes("see [site]\n\n> quoted [site]\n\n[site]: https://example.com\n");
        assert_eq!(parts.len(), 3);
        for part in &parts {
            let (Segment::Body(text) | Segment::Quote(text)) = part;
            assert!(text.contains("[site]: https://example.com"), "{text:?}");
        }
    }
}
