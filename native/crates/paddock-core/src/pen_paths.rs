//! Pen 데스크톱 앱 연동의 순수 규칙: `.pen` 파일 판정, 경로 표기 변환, 활성 문서 표기 비교, 설치 위치 후보, 열기 실패 문구.
//!
//! Pen은 Windows 프로그램이라 어떤 환경의 Claude가 준 경로든 Windows가 읽는 경로(`C:\…`·`\\wsl.localhost\…`)로 바꿔 넘겨야 한다.
//! 앱과 MCP 프로그램(`paddock-pen-mcp`)이 같은 규칙을 쓰도록 운영체제 호출 없이 글자만 다룬다.

use crate::file_kind::extension_of;

/// `.pen` 파일인지(확장자 비교, 대소문자 무관). `.pen` 파일은 탭으로 열지 않고 Pen 앱에 넘긴다.
///
/// Examples
/// --------
/// | 경로               | 결과  |
/// |--------------------|-------|
/// | `/w/design.pen`    | true  |
/// | `C:\w\Design.PEN`  | true  |
/// | `/w/design.pen.md` | false |
/// | `/w/.pen`          | false |
pub fn is_pen_file(path: &str) -> bool {
    extension_of(path) == "pen"
}

/// 경로를 준 Claude가 도는 환경. 같은 `/home/…` 표기가 환경에 따라 다른 파일을 가리키므로 변환 규칙이 갈린다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenHost {
    /// Windows 네이티브 Claude. `/…` 경로는 WSL 배포판 안의 경로로 본다.
    Windows,
    /// WSL 안의 Claude. `/mnt/<드라이브>/…`는 Windows 드라이브이고 나머지 `/…`는 배포판 안의 경로다.
    Wsl,
}

/// Pen에 넘길 파일 경로 두 가지. 같은 파일을 가리킨다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PenTarget {
    /// MCP 프로그램(Windows 프로세스)이 파일을 읽는 경로.
    pub local: String,
    /// Pen에 인자로 넘기고 Pencil 도구의 `filePath`로 쓰는 경로.
    pub windows: String,
}

/// 경로를 Pen 대상으로 바꿀 수 없는 이유.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PenPathError {
    /// 절대 경로가 아니다.
    NotAbsolute,
    /// `/…` 경로인데 어느 WSL 배포판의 경로인지 모른다.
    MissingDistribution,
}

impl std::fmt::Display for PenPathError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::NotAbsolute => formatter.write_str("Use an absolute path (a relative path has no known folder)."),
            Self::MissingDistribution => {
                formatter.write_str("This path is inside WSL, but the WSL distribution is unknown. Use a Windows path.")
            }
        }
    }
}

/// `C:` 같은 드라이브 글자로 시작하는지.
fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 슬래시를 역슬래시로 맞춘 Windows 표기. 드라이브 글자는 대문자로 둔다.
fn windows_spelling(path: &str) -> String {
    let mut spelled = path.replace('/', "\\");
    if has_drive(&spelled) {
        spelled[..1].make_ascii_uppercase();
    }
    spelled
}

/// 입력 경로를 Pen이 읽는 Windows 경로로 바꾼다.
///
/// 어느 환경의 Claude가 준 경로든 같은 파일이 같은 철자로 나와야 활성 문서와 창 제목을 비교할 수 있어서, 변환을 이 한 곳에 둔다.
/// 절대 경로가 아니면 `NotAbsolute`이고, `/…` 경로에 배포판 이름이 없으면 `MissingDistribution`이다.
/// `\\wsl$\…`는 `\\wsl.localhost\…`로 바꾼다(Pen이 앞의 표기를 못 읽는다). `.`·`..` 조각과 겹친 구분자는 푼다(Pen이 활성 문서를
/// 푼 철자로 알려 준다).
///
/// Examples
/// --------
/// | 입력                                  | host    | 배포판   | `windows`                          |
/// |---------------------------------------|---------|----------|------------------------------------|
/// | `C:\w\a.pen`·`C:/w/a.pen`             | Windows | -        | `C:\w\a.pen`                       |
/// | `\\wsl.localhost\Ubuntu\home\u\a.pen` | Windows | -        | 그대로                             |
/// | `\\wsl$\Ubuntu\home\u\a.pen`          | 모두    | -        | `\\wsl.localhost\Ubuntu\home\u\a.pen` |
/// | `/home/u/a.pen`                       | Wsl     | Ubuntu   | `\\wsl.localhost\Ubuntu\home\u\a.pen` |
/// | `/mnt/c/w/a.pen`                      | Wsl     | -        | `C:\w\a.pen`                       |
/// | `C:\w\..\w\a.pen`                     | Windows | -        | `C:\w\a.pen`                       |
/// | `/home/u/a.pen`                       | Wsl     | 없음     | 오류 `MissingDistribution`         |
/// | `a.pen`·`./a.pen`                     | 모두    | -        | 오류 `NotAbsolute`                 |
pub fn resolve_target(
    input: &str,
    host: PenHost,
    wsl_distribution: Option<&str>,
) -> Result<PenTarget, PenPathError> {
    let trimmed = input.trim();
    let windows = if has_drive(trimmed) && trimmed[2..].starts_with(['\\', '/']) {
        Ok(windows_spelling(trimmed))
    } else if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        let mut share = windows_spelling(trimmed);
        let host_end = share[2..].find('\\').map_or(share.len(), |at| at + 2);
        if share[2..host_end].eq_ignore_ascii_case("wsl$") || share[2..host_end].eq_ignore_ascii_case("wsl.localhost") {
            share.replace_range(2..host_end, "wsl.localhost");
        }
        Ok(share)
    } else if trimmed.starts_with('/') {
        let mount = trimmed.strip_prefix("/mnt/").filter(|_| host == PenHost::Wsl);
        let drive = mount.and_then(|rest| {
            let mut parts = rest.splitn(2, '/');
            let letter = parts.next().filter(|letter| letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic());
            letter.map(|letter| (letter.to_ascii_uppercase(), parts.next().unwrap_or("")))
        });
        match (drive, wsl_distribution.filter(|name| !name.is_empty())) {
            (Some((letter, rest)), _) => Ok(windows_spelling(&format!("{letter}:/{rest}"))),
            (None, Some(distribution)) => Ok(format!("\\\\wsl.localhost\\{distribution}{}", windows_spelling(trimmed))),
            (None, None) => Err(PenPathError::MissingDistribution),
        }
    } else {
        Err(PenPathError::NotAbsolute)
    };
    windows.map(|windows| {
        let windows = without_dot_segments(&windows);
        PenTarget { local: windows.clone(), windows }
    })
}

/// `.`·`..` 조각과 겹친 구분자를 글자만으로 푼다. 뿌리(`C:`·`\\호스트\공유`) 위로는 올라가지 않는다.
///
/// Pen은 활성 문서를 이 조각이 풀린 철자로 알려 주므로, 조각이 남은 경로로 열면 활성 문서와 맞지 않아 열기가 실패로 판정된다.
/// 공유 이름이 없는 `\\호스트`만의 경로는 그대로 둔다.
///
/// Examples
/// --------
/// | 입력                                     | 결과                                |
/// |------------------------------------------|-------------------------------------|
/// | `C:\w\..\w\.\a.pen`                      | `C:\w\a.pen`                        |
/// | `C:\..\a.pen`                            | `C:\a.pen`                          |
/// | `\\wsl.localhost\Ubuntu\..\tmp\\a.pen`   | `\\wsl.localhost\Ubuntu\tmp\a.pen`  |
fn without_dot_segments(windows_path: &str) -> String {
    let rooted = match windows_path.strip_prefix("\\\\") {
        Some(share) => {
            let mut parts = share.splitn(3, '\\');
            let (host, name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            (!name.is_empty()).then(|| (format!("\\\\{host}\\{name}"), parts.next().unwrap_or("")))
        }
        None if has_drive(windows_path) => Some((windows_path[..2].to_string(), &windows_path[2..])),
        None => None,
    };
    match rooted {
        Some((root, rest)) => {
            let mut kept: Vec<&str> = Vec::new();
            for piece in rest.split('\\') {
                match piece {
                    "" | "." => {}
                    ".." => {
                        kept.pop();
                    }
                    _ => kept.push(piece),
                }
            }
            format!("{root}\\{}", kept.join("\\"))
        }
        None => windows_path.to_string(),
    }
}

/// 활성 문서 표기(`/C:/…`·`/Ubuntu/…`)와 Windows 경로를 같은 꼴로 맞춘다: 역슬래시를 `/`로, 앞의 `//wsl.localhost`·`//wsl$`를 빼고,
/// `C:`를 `/C:`로 바꾼다. 대소문자는 파일 시스템에 맞춘다: 드라이브·공유 경로는 전체를 소문자로, WSL 배포판 안의 경로는 배포판 이름만
/// 소문자로 바꾸고 나머지는 그대로 둔다.
///
/// Pencil 도구는 활성 문서를 Windows 경로와 다른 철자로 알려 주므로, 열기 결과를 판정하려면 두 철자를 한 꼴로 맞춰 비교해야 한다.
/// WSL의 파일 시스템은 대소문자를 가리므로(`A.pen` ≠ `a.pen`) 그 부분까지 소문자로 바꾸면 다른 파일을 활성 문서로 오인한다.
///
/// Examples
/// --------
/// | 입력                                   | 결과                      |
/// |----------------------------------------|---------------------------|
/// | `C:\W\A.pen`                           | `/c:/w/a.pen`             |
/// | `/C:/w/a.pen`                          | `/c:/w/a.pen`             |
/// | `\\wsl.localhost\Ubuntu\home\u\A.pen`  | `/ubuntu/home/u/A.pen`    |
/// | `/Ubuntu/home/u/A.pen`                 | `/ubuntu/home/u/A.pen`    |
/// | `\\Server\Share\A.pen`                 | `//server/share/a.pen`    |
pub fn normalize_document(value: &str) -> String {
    let slashed = value.replace('\\', "/");
    let wsl_rest = ["//wsl.localhost/", "//wsl$/"].iter().find_map(|prefix| {
        let is_prefix = slashed.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix));
        is_prefix.then(|| &slashed[prefix.len() - 1..])
    });
    let is_drive_form = slashed.strip_prefix('/').is_some_and(has_drive);
    match wsl_rest {
        // 남은 부분은 활성 문서 표기 `/<배포판>/…`와 같은 꼴이다.
        Some(rest) => lower_first_piece(rest),
        None if has_drive(&slashed) => format!("/{}", slashed.to_ascii_lowercase()),
        None if is_drive_form || slashed.starts_with("//") => slashed.to_ascii_lowercase(),
        None => lower_first_piece(&slashed),
    }
}

/// `/<첫 조각>/<나머지>`에서 첫 조각(배포판 이름)만 소문자로 바꾼다.
fn lower_first_piece(rooted: &str) -> String {
    let body = rooted.strip_prefix('/').unwrap_or(rooted);
    let (first, rest) = body.find('/').map_or((body, ""), |at| body.split_at(at));
    format!("/{}{rest}", first.to_ascii_lowercase())
}

/// 활성 문서 표기(`/C:/…`·`/Ubuntu/…`)를 Windows 경로로 바꾼다.
///
/// 이전 활성 문서로 되돌아갈 때 그 문서를 Pen에 다시 열도록 Windows 경로가 필요하다.
///
/// Examples
/// --------
/// | 활성 문서 표기        | 결과                           |
/// |-----------------------|--------------------------------|
/// | `/C:/w/a.pen`         | `C:\w\a.pen`                   |
/// | `/Ubuntu/home/u/a.pen`| `\\wsl.localhost\Ubuntu\home\u\a.pen` |
pub fn document_to_windows(active: &str) -> String {
    let bytes = active.as_bytes();
    let is_drive_form = bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    if is_drive_form {
        active[1..].replace('/', "\\")
    } else {
        format!("\\\\wsl.localhost{}", active.replace('/', "\\"))
    }
}

/// Pen 설치 위치 후보(`Pen.exe` 경로), 먼저 찾을 것부터. 전체 사용자 설치와 현재 사용자 설치 두 곳이다.
///
/// Examples
/// --------
/// | program_files        | local_app_data                | 결과                                                                 |
/// |----------------------|-------------------------------|----------------------------------------------------------------------|
/// | `C:\Program Files`   | `C:\Users\me\AppData\Local`   | `C:\Program Files\Pen\Pen.exe`, `C:\Users\me\AppData\Local\Programs\Pen\Pen.exe` |
pub fn pen_install_candidates(
    program_files: &str,
    local_app_data: &str,
) -> Vec<String> {
    vec![
        format!("{}\\Pen\\Pen.exe", program_files.trim_end_matches('\\')),
        format!("{}\\Programs\\Pen\\Pen.exe", local_app_data.trim_end_matches('\\')),
    ]
}

/// Pen 설치 폴더 안의 Pencil MCP 프로그램 경로. Pen이 자기 문서를 다루는 도구를 이 프로그램으로 내놓는다.
///
/// Examples
/// --------
/// | pen_dir               | 결과                                                                                  |
/// |-----------------------|---------------------------------------------------------------------------------------|
/// | `C:\Program Files\Pen` | `C:\Program Files\Pen\resources\app.asar.unpacked\out\mcp-server-windows-x64.exe`     |
pub fn pencil_mcp_exe(pen_dir: &str) -> String {
    format!("{}\\resources\\app.asar.unpacked\\out\\mcp-server-windows-x64.exe", pen_dir.trim_end_matches('\\'))
}

/// `.pen` 파일을 Pen에 넘기지 못한 이유.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PenOpenFailure {
    /// `.pen` 연결도 설치 위치의 `Pen.exe`도 없다.
    NotInstalled,
    /// 열기 명령이나 `Pen.exe` 실행이 실패했다. 운영체제가 준 설명을 담는다.
    LaunchFailed(String),
}

/// 열기 실패를 상태 줄 한 줄로 쓴다.
///
/// Examples
/// --------
/// | 이유                    | 결과                                                       |
/// |-------------------------|------------------------------------------------------------|
/// | `NotInstalled`          | `Pen is not installed. Install Pen to open .pen files.`    |
/// | `LaunchFailed("denied")`| `Could not open the file in Pen: denied`                   |
pub fn pen_open_failure(reason: &PenOpenFailure) -> String {
    match reason {
        PenOpenFailure::NotInstalled => "Pen is not installed. Install Pen to open .pen files.".to_string(),
        PenOpenFailure::LaunchFailed(detail) => format!("Could not open the file in Pen: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_of(
        input: &str,
        host: PenHost,
        distribution: Option<&str>,
    ) -> Result<String, PenPathError> {
        resolve_target(input, host, distribution).map(|target| target.windows)
    }

    #[test]
    fn test_pen_1_pen_files_are_recognised_by_extension_only() {
        assert!(is_pen_file("/w/design.pen"));
        assert!(is_pen_file("C:\\w\\Design.PEN"));
        assert!(!is_pen_file("/w/design.pen.md"));
        assert!(!is_pen_file("/w/.pen"));
        assert!(!is_pen_file("/w/pen"));
    }

    #[test]
    fn test_pen_2_the_path_table_gives_the_windows_spelling_for_each_host() {
        // (입력, host, 배포판, 기대하는 Windows 경로 또는 오류)
        type Case = (&'static str, PenHost, Option<&'static str>, Result<&'static str, PenPathError>);
        let cases: [Case; 11] = [
            ("C:\\w\\a.pen", PenHost::Windows, None, Ok("C:\\w\\a.pen")),
            ("C:/w/a.pen", PenHost::Windows, None, Ok("C:\\w\\a.pen")),
            ("c:/w/a.pen", PenHost::Windows, None, Ok("C:\\w\\a.pen")),
            (
                "\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen",
                PenHost::Windows,
                None,
                Ok("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen"),
            ),
            ("\\\\wsl$\\Ubuntu\\home\\u\\a.pen", PenHost::Wsl, None, Ok("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen")),
            ("//WSL$/Ubuntu/home/u/a.pen", PenHost::Windows, None, Ok("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen")),
            ("/home/u/a.pen", PenHost::Wsl, Some("Ubuntu"), Ok("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen")),
            ("/mnt/c/w/a.pen", PenHost::Wsl, Some("Ubuntu"), Ok("C:\\w\\a.pen")),
            ("/home/u/a.pen", PenHost::Wsl, None, Err(PenPathError::MissingDistribution)),
            ("a.pen", PenHost::Wsl, Some("Ubuntu"), Err(PenPathError::NotAbsolute)),
            ("./a.pen", PenHost::Windows, None, Err(PenPathError::NotAbsolute)),
        ];
        for (input, host, distribution, expected) in cases {
            assert_eq!(windows_of(input, host, distribution), expected.map(str::to_string), "{input}");
        }
    }

    #[test]
    fn test_pen_3_a_mnt_path_is_a_drive_only_inside_wsl_and_local_equals_windows() {
        // Windows Claude에서 `/mnt/c/…`는 배포판 안의 경로다.
        assert_eq!(
            windows_of("/mnt/c/w/a.pen", PenHost::Windows, Some("Ubuntu")).unwrap(),
            "\\\\wsl.localhost\\Ubuntu\\mnt\\c\\w\\a.pen"
        );
        // `/mnt/cc/…`는 드라이브 글자가 아니다.
        assert_eq!(
            windows_of("/mnt/cc/a.pen", PenHost::Wsl, Some("Ubuntu")).unwrap(),
            "\\\\wsl.localhost\\Ubuntu\\mnt\\cc\\a.pen"
        );
        assert_eq!(windows_of("/mnt/D/x.pen", PenHost::Wsl, None).unwrap(), "D:\\x.pen");
        let target = resolve_target("/home/u/a b.pen", PenHost::Wsl, Some("Ubuntu")).unwrap();
        assert_eq!(target.local, target.windows);
        assert_eq!(windows_of("C:relative", PenHost::Windows, None), Err(PenPathError::NotAbsolute));
        assert_eq!(windows_of("", PenHost::Windows, None), Err(PenPathError::NotAbsolute));
    }

    #[test]
    fn test_pen_4_active_document_spellings_normalise_to_one_form() {
        assert_eq!(normalize_document("C:\\W\\A.pen"), "/c:/w/a.pen");
        assert_eq!(normalize_document("/C:/w/a.pen"), "/c:/w/a.pen");
        assert_eq!(normalize_document("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen"), "/ubuntu/home/u/a.pen");
        assert_eq!(normalize_document("\\\\wsl$\\Ubuntu\\home\\u\\a.pen"), "/ubuntu/home/u/a.pen");
        assert_eq!(normalize_document("/Ubuntu/home/u/a.pen"), "/ubuntu/home/u/a.pen");
        assert_eq!(
            normalize_document("\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen"),
            normalize_document("/Ubuntu/home/u/a.pen")
        );
        assert_eq!(normalize_document("\\\\Server\\Share\\A.pen"), "//server/share/a.pen");
    }

    #[test]
    fn test_pen_8_wsl_paths_keep_their_case_past_the_distribution_name() {
        // WSL 파일 시스템은 대소문자를 가린다: 대소문자만 다른 두 파일은 다른 문서다.
        assert_eq!(normalize_document("\\\\wsl.localhost\\Ubuntu\\home\\u\\A.pen"), "/ubuntu/home/u/A.pen");
        assert_eq!(normalize_document("/Ubuntu/home/u/A.pen"), "/ubuntu/home/u/A.pen");
        assert_ne!(normalize_document("/Ubuntu/home/u/a.pen"), normalize_document("/Ubuntu/home/u/A.pen"));
        // 배포판 이름과 공유 이름 표기는 대소문자를 가리지 않는다.
        assert_eq!(
            normalize_document("\\\\WSL.LOCALHOST\\ubuntu\\home\\u\\A.pen"),
            normalize_document("/Ubuntu/home/u/A.pen")
        );
        // Windows 드라이브는 대소문자를 가리지 않는다.
        assert_eq!(normalize_document("C:\\W\\A.pen"), normalize_document("/c:/w/a.PEN"));
    }

    #[test]
    fn test_pen_9_dot_segments_are_resolved_without_climbing_above_the_root() {
        assert_eq!(windows_of("C:\\w\\..\\w\\.\\a.pen", PenHost::Windows, None).unwrap(), "C:\\w\\a.pen");
        assert_eq!(windows_of("C:\\..\\a.pen", PenHost::Windows, None).unwrap(), "C:\\a.pen");
        assert_eq!(
            windows_of("\\\\wsl.localhost\\Ubuntu\\..\\tmp\\\\a.pen", PenHost::Windows, None).unwrap(),
            "\\\\wsl.localhost\\Ubuntu\\tmp\\a.pen"
        );
        assert_eq!(
            windows_of("/home/u/../v/./a.pen", PenHost::Wsl, Some("Ubuntu")).unwrap(),
            "\\\\wsl.localhost\\Ubuntu\\home\\v\\a.pen"
        );
        assert_eq!(windows_of("\\\\host", PenHost::Windows, None).unwrap(), "\\\\host");
    }

    #[test]
    fn test_pen_5_active_documents_convert_back_to_windows_paths() {
        assert_eq!(document_to_windows("/C:/w/a.pen"), "C:\\w\\a.pen");
        assert_eq!(document_to_windows("/Ubuntu/home/u/a.pen"), "\\\\wsl.localhost\\Ubuntu\\home\\u\\a.pen");
    }

    #[test]
    fn test_pen_6_install_candidates_and_the_pencil_program_follow_the_pen_folder() {
        assert_eq!(
            pen_install_candidates("C:\\Program Files", "C:\\Users\\me\\AppData\\Local\\"),
            vec![
                "C:\\Program Files\\Pen\\Pen.exe".to_string(),
                "C:\\Users\\me\\AppData\\Local\\Programs\\Pen\\Pen.exe".to_string(),
            ]
        );
        assert_eq!(
            pencil_mcp_exe("C:\\Program Files\\Pen"),
            "C:\\Program Files\\Pen\\resources\\app.asar.unpacked\\out\\mcp-server-windows-x64.exe"
        );
    }

    #[test]
    fn test_pen_7_failures_read_as_one_status_line() {
        assert_eq!(
            pen_open_failure(&PenOpenFailure::NotInstalled),
            "Pen is not installed. Install Pen to open .pen files."
        );
        assert_eq!(
            pen_open_failure(&PenOpenFailure::LaunchFailed("denied".into())),
            "Could not open the file in Pen: denied"
        );
    }
}
