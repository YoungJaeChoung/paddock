//! 터미널에 붙여 넣을 파일 경로 규칙: 경로를 그 터미널의 셸이 읽는 형식으로 바꾸고 감싸 한 줄로 잇는다. 클립보드 그림의 파일 이름·확장자도 정한다.
//! 탐색기에서 복사한 파일이나 저장한 클립보드 그림은 앱이 도는 운영체제의 경로라, WSL 셸에는 `/mnt/<드라이브>` 경로로 바꿔 넘긴다.
//! 화면과 파일 시스템을 모르는 순수 계산이다.
use crate::git_paths::parse_wsl_path;

/// 경로를 붙여 넣을 터미널의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PastePlace {
    /// Windows에서 도는 WSL 셸. Windows 경로를 `/mnt/…` 경로로 바꾼다.
    Wsl,
    /// PowerShell. 경로 철자는 그대로이고 특수 문자(`$`·백틱 포함)가 있으면 작은따옴표로 감싼다(안의 `'`는 `''`). 큰따옴표 안에서는 `$`·백틱이 풀리기 때문이다.
    PowerShell,
    /// 그 밖의 Windows 셸(cmd·Git Bash). 경로 철자는 그대로이고 특수 문자가 있으면 큰따옴표로 감싼다.
    Windows,
    /// Linux·macOS 셸. 특수 문자 앞에 `\`를 붙인다.
    Posix,
}

/// 셸 이름(`powershell`·`cmd`·`gitbash`·`wsl`·`bash`)과 운영체제 이름에서 경로 붙여넣기 종류를 정한다. Windows가 아니면 모두 `Posix`다.
///
/// Examples
/// --------
/// | 셸           | 운영체제  | 결과      |
/// |--------------|-----------|-----------|
/// | `wsl`        | `windows` | `Wsl`     |
/// | `powershell` | `windows` | `PowerShell` |
/// | `cmd`        | `windows` | `Windows` |
/// | `gitbash`    | `windows` | `Windows` |
/// | `bash`       | `linux`   | `Posix`   |
/// | `wsl`        | `linux`   | `Posix`   |
pub fn place_of(
    shell_name: &str,
    os: &str,
) -> PastePlace {
    match (os, shell_name) {
        ("windows", "wsl") => PastePlace::Wsl,
        ("windows", "powershell") => PastePlace::PowerShell,
        ("windows", _) => PastePlace::Windows,
        _ => PastePlace::Posix,
    }
}

/// Windows 경로를 WSL 셸에서 같은 파일을 가리키는 경로로 바꾼다. 바꿀 수 없는 형식은 그대로 둔다.
///
/// Examples
/// --------
/// | 경로                              | 결과                      |
/// |-----------------------------------|---------------------------|
/// | `C:\Users\me\shot.png`            | `/mnt/c/Users/me/shot.png`|
/// | `D:\`                             | `/mnt/d`                  |
/// | `\\wsl.localhost\Ubuntu\home\a.png` | `/home/a.png`           |
/// | `\\server\share\a.png`            | 그대로                    |
pub fn to_wsl_path(windows_path: &str) -> String {
    let bytes = windows_path.as_bytes();
    let is_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let on_drive_root = bytes.len() == 2 || (bytes.len() > 2 && (bytes[2] == b'\\' || bytes[2] == b'/'));
    if is_drive && on_drive_root {
        let rest = windows_path[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        let path = format!("/mnt/{}/{rest}", (bytes[0] as char).to_ascii_lowercase());
        path.trim_end_matches('/').to_string()
    } else if let Some((_, _, linux_path)) = parse_wsl_path(windows_path) {
        linux_path
    } else {
        windows_path.to_string()
    }
}

/// 셸이 한 단어로 읽도록 경로를 감싼다. PowerShell은 글자·숫자·`_-.:\/`가 아닌 글자가 있으면 작은따옴표로 감싸고(안의 `'`는 두 번 쓴다),
/// 그 밖의 Windows 셸(cmd·Git Bash)은 빈칸·`& ( ) ^ , ; = ! ' %`가 있으면 큰따옴표로 감싸며,
/// WSL·POSIX 셸은 글자·숫자·`_@%+=:,./~-`가 아닌 글자마다 앞에 `\`를 붙인다.
pub fn quote_path(
    path: &str,
    place: PastePlace,
) -> String {
    match place {
        PastePlace::PowerShell => {
            if path.chars().all(|c| c.is_alphanumeric() || "_-.:\\/".contains(c)) {
                path.to_string()
            } else {
                format!("'{}'", path.replace('\'', "''"))
            }
        }
        PastePlace::Windows => {
            if path.chars().any(|c| c.is_whitespace() || "&()^,;=!'%".contains(c)) {
                format!("\"{path}\"")
            } else {
                path.to_string()
            }
        }
        PastePlace::Wsl | PastePlace::Posix => path
            .chars()
            .flat_map(|c| {
                let plain = c.is_alphanumeric() || "_@%+=:,./~-".contains(c);
                if plain { vec![c] } else { vec!['\\', c] }
            })
            .collect(),
    }
}

/// 붙여 넣을 파일 경로 목록을 터미널 입력 한 줄로 만든다. 경로마다 터미널 종류에 맞게 바꾸고 감싼 뒤 빈칸으로 잇는다. 경로가 없으면 빈 글자다.
/// 줄바꿈 같은 제어 문자가 든 경로가 하나라도 있으면 None이다 — 그 글자는 셸이 명령 경계로 읽어 붙여 넣기가 명령 실행이 될 수 있어 아무것도 붙이지 않는다.
///
/// Examples
/// --------
/// | 경로                    | 터미널  | 결과                       |
/// |-------------------------|---------|----------------------------|
/// | `C:\Users\me\shot.png`  | `Wsl`   | `/mnt/c/Users/me/shot.png` |
/// | `C:\My Files\a.png`     | `Wsl`   | `/mnt/c/My\ Files/a.png`   |
/// | `C:\My Files\a.png`     | `PowerShell` | `'C:\My Files\a.png'` |
/// | `C:\it's $x.png`         | `PowerShell` | `'C:\it''s $x.png'`    |
/// | `C:\My Files\a.png`     | `Windows` | `"C:\My Files\a.png"`    |
/// | `/a\nb.png`              | `Posix`   | None(제어 문자)             |
/// | `/Users/me/My Shot.png` | `Posix` | `/Users/me/My\ Shot.png`   |
pub fn paste_path_text(
    paths: &[String],
    place: PastePlace,
) -> Option<String> {
    let has_control = paths.iter().any(|path| path.chars().any(char::is_control));
    (!has_control).then(|| {
        paths
            .iter()
            .map(|path| {
                let shown = if place == PastePlace::Wsl { to_wsl_path(path) } else { path.clone() };
                quote_path(&shown, place)
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// 제어 문자가 든 경로를 붙이지 않았을 때 상태 줄에 보이는 안내.
pub const CONTROL_CHARACTER_NOTICE: &str =
    "A file path contains a line break or control character, so it was not pasted.";

/// 클립보드 그림의 형식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
    Bmp,
    /// 앱이 이름을 아는 형식이 아니다(TIFF·ICO 등).
    Other,
}

impl ImageKind {
    /// 파일로 쓸 확장자. 모르는 형식과 BMP는 `png`다(PNG로 다시 인코딩해 쓴다 — BMP는 투명도가 없어 파일로 남기기에 부적합하다).
    ///
    /// Examples
    /// --------
    /// | 형식   | 확장자 |
    /// |--------|--------|
    /// | Png    | `png`  |
    /// | Jpeg   | `jpg`  |
    /// | Gif    | `gif`  |
    /// | Webp   | `webp` |
    /// | Svg    | `svg`  |
    /// | Bmp·Other | `png` |
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::Bmp | Self::Other => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
        }
    }

    /// 바이트를 PNG로 다시 인코딩해야 하는 형식인지(BMP·모르는 형식).
    pub fn needs_png_encoding(self) -> bool {
        matches!(self, Self::Bmp | Self::Other)
    }
}

/// 터미널에 붙여 넣은 그림을 저장할 파일 이름 `paste-<YYYYMMDD>-<HHMMSS>-<난수>.<확장자>`. 시각은 UTC다.
///
/// Examples
/// --------
/// | 유닉스 초    | 난수   | 확장자 | 결과                           |
/// |--------------|--------|--------|--------------------------------|
/// | 1792855812   | `a1b2` | `png`  | `paste-20261024-153012-a1b2.png` |
pub fn pasted_image_name(
    unix_seconds: u64,
    random: &str,
    extension: &str,
) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let of_day = unix_seconds % 86_400;
    // 유닉스 날짜를 그레고리력 날짜로(Howard Hinnant의 civil_from_days).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "paste-{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{random}.{extension}",
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d69_windows_paths_become_the_wsl_shells_paths() {
        assert_eq!(to_wsl_path("C:\\Users\\me\\shot.png"), "/mnt/c/Users/me/shot.png");
        assert_eq!(to_wsl_path("D:\\"), "/mnt/d");
        assert_eq!(to_wsl_path("\\\\wsl.localhost\\Ubuntu\\home\\me\\a.png"), "/home/me/a.png");
        assert_eq!(to_wsl_path("\\\\wsl$\\Ubuntu\\tmp"), "/tmp");
        assert_eq!(to_wsl_path("\\\\server\\share\\a.png"), "\\\\server\\share\\a.png");
        assert_eq!(to_wsl_path("/already/linux"), "/already/linux");
    }

    #[test]
    fn test_c_d69_terminal_paste_of_files_puts_wsl_paths_for_a_wsl_shell() {
        let paths = |list: &[&str]| list.iter().map(|path| path.to_string()).collect::<Vec<_>>();
        let text = |list: &[&str], place| paste_path_text(&paths(list), place);
        assert_eq!(
            text(&["C:\\My Files\\a.png", "C:\\b.png"], PastePlace::Wsl).as_deref(),
            Some("/mnt/c/My\\ Files/a.png /mnt/c/b.png")
        );
        // cmd·Git Bash는 큰따옴표, PowerShell은 작은따옴표다(큰따옴표 안에서는 `$`·백틱이 풀린다).
        assert_eq!(text(&["C:\\My Files\\a.png"], PastePlace::Windows).as_deref(), Some("\"C:\\My Files\\a.png\""));
        assert_eq!(text(&["C:\\My Files\\a.png"], PastePlace::PowerShell).as_deref(), Some("'C:\\My Files\\a.png'"));
        assert_eq!(text(&["C:\\it's $x.png"], PastePlace::PowerShell).as_deref(), Some("'C:\\it''s $x.png'"));
        assert_eq!(text(&["C:\\a`b.png"], PastePlace::PowerShell).as_deref(), Some("'C:\\a`b.png'"));
        assert_eq!(text(&["C:\\a.png"], PastePlace::Windows).as_deref(), Some("C:\\a.png"));
        assert_eq!(
            text(&["C:\\a.png", "D:/b-c_d.txt"], PastePlace::PowerShell).as_deref(),
            Some("C:\\a.png D:/b-c_d.txt")
        );
        assert_eq!(
            text(&["/Users/me/it's (1).png"], PastePlace::Posix).as_deref(),
            Some("/Users/me/it\\'s\\ \\(1\\).png")
        );
        assert_eq!(text(&["/home/me/한글.png"], PastePlace::Posix).as_deref(), Some("/home/me/한글.png"));
        assert_eq!(text(&[], PastePlace::Posix).as_deref(), Some(""));
    }

    #[test]
    fn test_c_d69_a_path_with_a_line_break_or_control_character_is_not_pasted_at_all() {
        let paths = |list: &[&str]| list.iter().map(|path| path.to_string()).collect::<Vec<_>>();
        for place in [PastePlace::Wsl, PastePlace::PowerShell, PastePlace::Windows, PastePlace::Posix] {
            assert_eq!(paste_path_text(&paths(&["/ok.png", "/bad\nname.png"]), place), None, "{place:?}");
            assert_eq!(paste_path_text(&paths(&["/tab\tname.png"]), place), None, "{place:?}");
            assert_eq!(paste_path_text(&paths(&["/bell\u{7}.png"]), place), None, "{place:?}");
        }
        assert!(CONTROL_CHARACTER_NOTICE.contains("line break or control character"));
    }

    #[test]
    fn test_c_d69_image_formats_pick_their_extension_and_unknown_ones_are_reencoded_as_png() {
        for (kind, extension) in [
            (ImageKind::Png, "png"),
            (ImageKind::Jpeg, "jpg"),
            (ImageKind::Gif, "gif"),
            (ImageKind::Webp, "webp"),
            (ImageKind::Svg, "svg"),
            (ImageKind::Bmp, "png"),
            (ImageKind::Other, "png"),
        ] {
            assert_eq!(kind.extension(), extension, "{kind:?}");
        }
        assert!(ImageKind::Bmp.needs_png_encoding() && ImageKind::Other.needs_png_encoding());
        assert!(!ImageKind::Png.needs_png_encoding() && !ImageKind::Jpeg.needs_png_encoding());
    }

    #[test]
    fn test_c_d69_the_shell_kind_decides_how_paths_are_pasted() {
        assert_eq!(place_of("wsl", "windows"), PastePlace::Wsl);
        assert_eq!(place_of("powershell", "windows"), PastePlace::PowerShell);
        assert_eq!(place_of("cmd", "windows"), PastePlace::Windows);
        assert_eq!(place_of("bash", "linux"), PastePlace::Posix);
        assert_eq!(place_of("wsl", "macos"), PastePlace::Posix);
    }

    #[test]
    fn test_c_d69_a_pasted_image_name_carries_the_utc_time_and_the_random_part() {
        assert_eq!(pasted_image_name(0, "zzzz", "png"), "paste-19700101-000000-zzzz.png");
        assert_eq!(pasted_image_name(1_792_855_812, "a1b2", "jpg"), "paste-20261024-153012-a1b2.jpg");
        assert_eq!(
            pasted_image_name(951_782_399, "x", "png"),
            "paste-20000228-235959-x.png",
            "the day before a leap day"
        );
        assert_eq!(
            pasted_image_name(951_868_800, "x", "png"),
            "paste-20000301-000000-x.png",
            "the day after a leap day"
        );
        assert_eq!(pasted_image_name(951_782_400, "x", "png"), "paste-20000229-000000-x.png");
    }
}
