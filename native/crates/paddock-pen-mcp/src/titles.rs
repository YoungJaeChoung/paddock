//! 창 제목으로 파일의 Pen 창을 가르는 순수 규칙. Pen은 파일마다 창을 열고 창 제목이 파일 이름이다.

/// 파일 이름으로 창을 찾은 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum Found<T> {
    None,
    One(T),
    /// 폴더가 달라 이름이 같은 창이 둘 이상이라 어느 파일인지 가를 수 없다.
    Ambiguous,
}

/// Windows 경로의 마지막 조각(파일 이름).
///
/// Examples
/// --------
/// | 경로                                  | 결과    |
/// |---------------------------------------|---------|
/// | `C:\w\a.pen`                          | `a.pen` |
/// | `\\wsl.localhost\Ubuntu\home\u\b.pen` | `b.pen` |
pub fn file_name_of(windows_path: &str) -> &str {
    windows_path.rsplit('\\').next().unwrap_or(windows_path)
}

/// 창 제목이 파일 이름과 같은지. Windows 파일 이름은 대소문자를 가리지 않으므로 대소문자만 다른 이름은 같은 이름이다.
///
/// Examples
/// --------
/// | 제목     | 파일 이름 | 결과  |
/// |----------|-----------|-------|
/// | `a.pen`  | `a.pen`   | true  |
/// | `A.PEN`  | `a.pen`   | true  |
/// | `a.pen*` | `a.pen`   | false |
pub fn title_matches(
    title: &str,
    file_name: &str,
) -> bool {
    title.to_lowercase() == file_name.to_lowercase()
}

/// 제목이 파일 이름과 같은 창들을 한 창·없음·여럿으로 가른다. 대소문자만 다른 이름의 창이 둘이면 여럿이다.
///
/// 창을 못 찾았다는 판정(`None`)은 같은 이름의 창이 하나도 없을 때만 나온다. 그래서 `pen_close`의 "이미 닫혀 있음"이 다른 대소문자의 창을 두고 나가지 않는다.
///
/// Examples
/// --------
/// | 창 제목들             | 파일 이름 | 결과         |
/// |-----------------------|-----------|--------------|
/// | (없음)                | `a.pen`   | `None`       |
/// | `a.pen`               | `a.pen`   | `One`        |
/// | `A.pen`               | `a.pen`   | `One`        |
/// | `a.pen`, `A.PEN`      | `a.pen`   | `Ambiguous`  |
/// | `b.pen`               | `a.pen`   | `None`       |
pub fn find_by_title<T: Copy>(
    windows: &[(T, String)],
    file_name: &str,
) -> Found<T> {
    let matches: Vec<T> =
        windows.iter().filter(|(_, title)| title_matches(title, file_name)).map(|(window, _)| *window).collect();
    match matches.as_slice() {
        [] => Found::None,
        [only] => Found::One(*only),
        _ => Found::Ambiguous,
    }
}

/// 이름이 같은 창 하나가 대상의 창인지, 대상을 활성 문서로 만들기 전후의 문서 창으로 가른 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum Identified<T> {
    /// 대상은 이미 그 창에 열려 있었다(활성으로 만들어도 새 창이 생기지 않았다).
    Open(T),
    /// 대상은 열려 있지 않았다. 활성으로 만들며 새로 열린 이 창이 대상의 창이고, 이름이 같던 창은 다른 파일의 창이다.
    OpenedForCheck(T),
    /// 어느 쪽인지 가를 수 없다(새 창이 대상 이름이 아니거나 여럿, 이름이 같은 창이 사라지거나 늘어남).
    Unknown,
}

/// 창 제목에는 파일 이름만 있어, 이름이 같은 창 하나(`named`)가 대상의 창인지는 제목만으로 알 수 없다. 대상을 활성 문서로 만들면
/// 열려 있던 파일은 그 창이 쓰이고, 열려 있지 않던 파일은 새 창이 열린다. 그래서 만들기 전의 문서 창(`before`)과 만든 뒤의 문서 창·제목
/// (`after`)을 견주어 가른다.
///
/// Examples
/// --------
/// | named | before    | after                         | 결과                  |
/// |-------|-----------|-------------------------------|-----------------------|
/// | 1     | 0, 1      | (0 Dashboard), (1 a.pen)      | `Open(1)`             |
/// | 1     | 0, 1      | 0, (1 a.pen), (2 a.pen)       | `OpenedForCheck(2)`   |
/// | 1     | 0, 1      | 0, (1 a.pen), (2 Pen)         | `Unknown`(제목 전)    |
/// | 1     | 0, 1      | 0, (1 a.pen), 2, 3            | `Unknown`             |
pub fn identify_target<T: Copy + PartialEq>(
    named: T,
    before: &[T],
    after: &[(T, String)],
    file_name: &str,
) -> Identified<T> {
    let new: Vec<T> = after.iter().map(|(window, _)| *window).filter(|window| !before.contains(window)).collect();
    let named_after: Vec<T> =
        after.iter().filter(|(_, title)| title_matches(title, file_name)).map(|(window, _)| *window).collect();
    match (new.as_slice(), named_after.as_slice()) {
        ([], [only]) if *only == named => Identified::Open(named),
        ([opened], [first, second])
            if (*first, *second) == (named, *opened) || (*first, *second) == (*opened, named) =>
        {
            Identified::OpenedForCheck(*opened)
        }
        _ => Identified::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(titles: &[&str]) -> Vec<(u32, String)> {
        titles.iter().enumerate().map(|(index, title)| (index as u32, title.to_string())).collect()
    }

    #[test]
    fn test_titles_1_file_names_come_from_the_last_path_piece() {
        assert_eq!(file_name_of("C:\\w\\a.pen"), "a.pen");
        assert_eq!(file_name_of("\\\\wsl.localhost\\Ubuntu\\home\\u\\b.pen"), "b.pen");
        assert_eq!(file_name_of("a.pen"), "a.pen");
    }

    #[test]
    fn test_titles_2_titles_match_whatever_the_case_but_not_extra_marks() {
        assert!(title_matches("A.PEN", "a.pen"));
        assert!(title_matches("한글.pen", "한글.PEN"));
        assert!(!title_matches("a.pen*", "a.pen"));
    }

    #[test]
    fn test_titles_3_windows_that_differ_only_in_case_are_ambiguous_and_one_of_another_case_is_found() {
        assert_eq!(find_by_title(&titled(&[]), "a.pen"), Found::None);
        assert_eq!(find_by_title(&titled(&["b.pen"]), "a.pen"), Found::None);
        assert_eq!(find_by_title(&titled(&["a.pen", "b.pen"]), "a.pen"), Found::One(0));
        // 다른 대소문자의 창 하나는 "없음"이 아니라 그 창이다 — 닫기가 이미 닫혔다고 거짓으로 보고하지 않는다.
        assert_eq!(find_by_title(&titled(&["A.pen"]), "a.pen"), Found::One(0));
        assert_eq!(find_by_title(&titled(&["a.pen", "A.PEN"]), "a.pen"), Found::Ambiguous);
        assert_eq!(find_by_title(&titled(&["a.pen", "a.pen"]), "A.pen"), Found::Ambiguous);
    }

    #[test]
    fn test_titles_4_a_window_is_the_target_only_if_making_the_target_active_opened_no_new_window() {
        let titled = |pairs: &[(u32, &str)]| {
            pairs.iter().map(|(window, title)| (*window, title.to_string())).collect::<Vec<_>>()
        };
        assert_eq!(
            identify_target(1, &[0, 1], &titled(&[(0, "Dashboard"), (1, "a.pen")]), "a.pen"),
            Identified::Open(1)
        );
        assert_eq!(
            identify_target(1, &[0, 1], &titled(&[(0, "Dashboard"), (1, "a.pen"), (2, "a.pen")]), "a.pen"),
            Identified::OpenedForCheck(2)
        );
        assert_eq!(
            identify_target(1, &[0, 1], &titled(&[(2, "A.PEN"), (0, "Dashboard"), (1, "a.pen")]), "a.pen"),
            Identified::OpenedForCheck(2)
        );
        // 새 창의 제목이 아직 파일 이름이 아니면 가를 수 없다(호출자가 다시 본다).
        assert_eq!(
            identify_target(1, &[0, 1], &titled(&[(0, "Dashboard"), (1, "a.pen"), (2, "Pen")]), "a.pen"),
            Identified::Unknown
        );
        assert_eq!(
            identify_target(1, &[0, 1], &titled(&[(0, "x"), (1, "a.pen"), (2, "a.pen"), (3, "a.pen")]), "a.pen"),
            Identified::Unknown
        );
        // 이름이 같던 창이 사라졌으면 가를 수 없다.
        assert_eq!(identify_target(1, &[0, 1], &titled(&[(0, "Dashboard")]), "a.pen"), Identified::Unknown);
    }
}
