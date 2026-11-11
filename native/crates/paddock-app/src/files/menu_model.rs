//! 파일 목록 우클릭 메뉴의 규칙: 항목 이름과 "파일 관리자에서 보기"가 띄울 프로그램. 화면을 모르는 순수 규칙이다.

/// 파일 관리자에서 항목을 보여 주는 메뉴 항목의 글자. `os`는 `std::env::consts::OS` 값이다.
///
/// Examples
/// --------
/// | os        | 결과                       |
/// |-----------|----------------------------|
/// | `windows` | `Reveal in File Explorer`  |
/// | `macos`   | `Reveal in Finder`         |
/// | `linux`   | `Reveal in File Manager`   |
pub fn reveal_label(os: &str) -> &'static str {
    match os {
        "windows" => "Reveal in File Explorer",
        "macos" => "Reveal in Finder",
        _ => "Reveal in File Manager",
    }
}

/// 창 제목. 어느 폴더에 생기는지 보이게 한다.
///
/// Examples
/// --------
/// | is_dir | 폴더 라벨        | 결과                       |
/// |--------|------------------|----------------------------|
/// | false  | `shop-api/src`   | `New File in shop-api/src` |
/// | true   | `shop-api`       | `New Folder in shop-api`   |
pub fn entry_title(
    is_dir: bool,
    label: &str,
) -> String {
    format!("{} in {label}", if is_dir { "New Folder" } else { "New File" })
}

/// 항목을 파일 관리자에서 보여 줄 프로그램과 인자. 셸을 거치지 않고 인자 배열로 띄운다.
///
/// Windows와 macOS는 관리자 창에서 그 항목을 선택해 보여 주고, Linux는 선택 기능이 표준이 아니라 부모 폴더를 연다.
///
/// Examples
/// --------
/// | os        | 경로                    | 프로그램       | 인자                       |
/// |-----------|-------------------------|----------------|----------------------------|
/// | `windows` | `C:/Users/me/a.txt`     | `explorer.exe` | `/select,C:\Users\me\a.txt` |
/// | `macos`   | `/Users/me/a.txt`       | `open`         | `-R`, `/Users/me/a.txt`    |
/// | `linux`   | `/home/me/a.txt`        | `xdg-open`     | `/home/me`                 |
pub fn reveal_command(
    os: &str,
    path: &str,
) -> (String, Vec<String>) {
    match os {
        "windows" => ("explorer.exe".to_string(), vec![format!("/select,{}", path.replace('/', "\\"))]),
        "macos" => ("open".to_string(), vec!["-R".to_string(), path.to_string()]),
        _ => {
            let parent = path.trim_end_matches('/').rsplit_once('/').map_or("/", |(parent, _)| parent);
            ("xdg-open".to_string(), vec![if parent.is_empty() { "/" } else { parent }.to_string()])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_files_24_the_new_entry_window_title_names_the_folder_it_will_create_in() {
        assert_eq!(entry_title(false, "shop-api/src"), "New File in shop-api/src");
        assert_eq!(entry_title(true, "shop-api"), "New Folder in shop-api");
    }

    #[test]
    fn test_c_files_20_reveal_label_and_command_follow_the_operating_system() {
        assert_eq!(reveal_label("windows"), "Reveal in File Explorer");
        assert_eq!(reveal_label("macos"), "Reveal in Finder");
        assert_eq!(reveal_label("linux"), "Reveal in File Manager");
        let (program, args) = reveal_command("windows", "C:/Users/me/a.txt");
        assert_eq!((program.as_str(), args), ("explorer.exe", vec![r"/select,C:\Users\me\a.txt".to_string()]));
        let (program, args) = reveal_command("windows", r"\\wsl.localhost\Ubuntu\home\a.txt");
        assert_eq!(
            (program.as_str(), args),
            ("explorer.exe", vec![r"/select,\\wsl.localhost\Ubuntu\home\a.txt".to_string()])
        );
        let (program, args) = reveal_command("macos", "/Users/me/a.txt");
        assert_eq!((program.as_str(), args), ("open", vec!["-R".to_string(), "/Users/me/a.txt".to_string()]));
        let (program, args) = reveal_command("linux", "/home/me/a b/c.txt");
        assert_eq!((program.as_str(), args), ("xdg-open", vec!["/home/me/a b".to_string()]));
    }
}
