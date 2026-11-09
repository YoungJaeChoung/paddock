//! 폴더 검색이 훑을 폴더를 정하는 규칙. 판정만 두어 창 없이 시험한다.
use crate::file_tree::files_folder;

/// 검색할 폴더. 파일 구획이 보여 주는 폴더가 먼저이고, 없으면 선택한 터미널의 현재 폴더이며, 둘 다 없으면 None이다(검색할 곳이 없다).
///
/// 검색은 작업 폴더 하나를 훑는다. 파일 구획과 같은 폴더를 훑어야 목록에서 보이던 파일이 결과에 나온다.
/// 터미널의 현재 폴더는 파일 구획이 폴더를 고를 때와 같은 규칙(`files_folder`)으로 거른다 — 홈 폴더(WSL 배포판의 홈 포함)와 파일 시스템 루트는
/// 프로젝트가 아니라서 통째로 훑으면 몇 분이 걸리므로 검색할 폴더가 없는 것으로 본다.
///
/// Examples
/// --------
/// | 파일 구획 폴더 | 터미널 현재 폴더 | 홈          | 결과        |
/// |----------------|------------------|-------------|-------------|
/// | `/w/shop`      | `/w/shop/src`    | -           | `/w/shop`   |
/// | (없음)         | `/tmp/c/`        | -           | `/tmp/c`    |
/// | (없음)         | 홈 폴더          | `/home/me`  | None        |
/// | (없음)         | `/`·`C:\`        | -           | None        |
/// | (없음)         | (없음)           | -           | None        |
pub fn search_folder(
    files_folder_shown: Option<&str>,
    terminal_cwd: Option<&str>,
    home: Option<&str>,
) -> Option<String> {
    files_folder_shown
        .filter(|folder| !folder.is_empty())
        .map(str::to_string)
        .or_else(|| files_folder(None, terminal_cwd, None, home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ss02_the_folder_of_the_file_area_is_searched_first() {
        assert_eq!(search_folder(Some("/w/shop"), Some("/w/shop/src"), None), Some("/w/shop".to_string()));
    }

    #[test]
    fn test_ss03_without_a_file_area_the_current_folder_of_the_selected_terminal_is_searched() {
        assert_eq!(search_folder(None, Some("/tmp/c"), None), Some("/tmp/c".to_string()));
    }

    #[test]
    fn test_ss04_with_no_folder_at_all_there_is_nothing_to_search() {
        assert_eq!(search_folder(None, None, None), None);
    }

    #[test]
    fn test_the_home_folder_and_the_file_system_root_are_not_searched() {
        let home = Some("/home/me");
        assert_eq!(search_folder(None, Some("/home/me"), home), None);
        assert_eq!(search_folder(None, Some("/"), home), None);
        assert_eq!(search_folder(None, Some("C:\\"), None), None);
        assert_eq!(search_folder(None, Some("\\\\wsl.localhost\\Ubuntu\\home\\me"), None), None);
        assert_eq!(
            search_folder(None, Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\app"), None).as_deref(),
            Some("\\\\wsl.localhost\\Ubuntu\\home\\me\\app")
        );
        assert_eq!(search_folder(None, Some("/home/me/app"), home).as_deref(), Some("/home/me/app"));
    }
}
