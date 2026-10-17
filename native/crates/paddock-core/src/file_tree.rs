//! 사이드바 파일 목록(`<폴더> FILES`)의 규칙: 숨길 이름, 정렬, 목록이 보여 줄 폴더, 새 항목이 만들어질 폴더, 폴더 라벨, 화면에 그릴 행.
//!
//! 디스크를 읽는 일은 앱이 하고, 여기서는 읽은 결과를 받아 화면 행으로 바꾸는 순수 규칙만 둔다.
use std::collections::HashSet;

use crate::settings_text::SettingValues;
use crate::work_model::{Folder, folder_key, folder_name, is_inside, same_folder};

/// 폴더를 읽지 못했을 때 목록 자리에 보이는 안내.
pub const NOT_FOUND_NOTICE: &str = "Folder not found.";

/// 폴더가 비었을 때 목록 자리에 보이는 안내.
pub const EMPTY_NOTICE: &str = "Empty folder.";

/// 폴더 안 항목 하나. 폴더이고 펼쳐져 있으면 `directory`에 그 안을 읽은 결과가 들어 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub directory: Option<Box<Directory>>,
}

/// 폴더 하나를 읽은 결과. `found`가 거짓이면 읽지 못한 폴더(없거나 권한이 없음)이고, `empty`는 폴더 안에 이름이 하나도 없다는 뜻이다(숨긴 이름만 든 폴더는 비지 않았다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory {
    pub found: bool,
    pub empty: bool,
    pub children: Vec<Entry>,
}

/// 목록에 그릴 행 하나. `notice`가 있으면 항목이 아니라 그 깊이에 보이는 안내 글이다(이때 `name`은 비어 있다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub notice: Option<&'static str>,
}

/// 목록에서 숨길 이름. `files.exclude` 설정에서 켜진 패턴 중 이름 하나로 된 것이다.
///
/// 앞에 붙은 "모든 하위 폴더" 표지(`**/`)는 떼고 본다. 와일드카드(`*`·`?`·`[`·`{`)나 경로 구분자가 든 패턴은 다루지 않는다.
/// 설정에 아무것도 없으면 기본 일곱 이름(`.git`·`.svn`·`.hg`·`CVS`·`.DS_Store`·`Thumbs.db`·`__pycache__`)이다.
///
/// Examples
/// --------
/// | `files.exclude`                  | 결과                                |
/// |----------------------------------|-------------------------------------|
/// | (없음)                           | 기본 일곱 이름                      |
/// | `{"**/node_modules": true}`      | 기본 일곱 이름 + `node_modules`     |
/// | `{"**/.git": false}`             | 기본에서 `.git`만 뺀 여섯 이름      |
/// | `{"**/*.pyc": true}`             | 기본 일곱 이름(와일드카드는 무시)   |
pub fn hidden_names(settings: &SettingValues) -> HashSet<String> {
    settings
        .files_exclude()
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(pattern, _)| pattern.strip_prefix("**/").unwrap_or(&pattern).to_string())
        .filter(|name| !name.is_empty() && !name.contains(['*', '?', '[', ']', '{', '}', '/', '\\']))
        .collect()
}

/// ASCII 문장부호의 순서(기존 앱이 쓰던 로케일 비교와 같다): 공백이 가장 앞이고 그다음 `_` `-` `,` … `$` 순이며, 숫자와 글자는 모두 이들 뒤다.
const PUNCTUATION_ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// 글자 하나의 비교 열쇠: (종류, 순위). 종류는 문장부호 < 숫자 < 영문자(대소문자 무시) < 그 밖의 글자(코드 포인트)다.
fn name_key(c: char) -> (u8, u32) {
    match PUNCTUATION_ORDER.find(c) {
        Some(index) => (0, index as u32),
        None if c.is_ascii_digit() => (1, u32::from(c)),
        None if c.is_ascii_alphabetic() => (2, u32::from(c.to_ascii_lowercase())),
        None if c.is_ascii() => (0, 0),
        None => (3, c.to_lowercase().next().map_or(0, u32::from)),
    }
}

/// 이름 두 개의 순서. 대소문자를 가리지 않고 비교하고, 동률이면 처음 다른 글자에서 소문자가 먼저이며, 그래도 같으면 글자 그대로 비교한다.
///
/// 한계: ASCII가 아닌 글자는 악센트·언어 규칙 없이 코드 포인트로 비교한다(`É`는 `e` 곁이 아니라 `z` 뒤에 온다).
fn compare_names(
    left: &str,
    right: &str,
) -> std::cmp::Ordering {
    left.chars()
        .map(name_key)
        .cmp(right.chars().map(name_key))
        .then_with(|| {
            let first_difference = left.chars().zip(right.chars()).find(|(a, b)| a != b);
            match first_difference {
                Some((a, _)) if a.is_lowercase() => std::cmp::Ordering::Less,
                Some(_) => std::cmp::Ordering::Greater,
                None => std::cmp::Ordering::Equal,
            }
        })
        .then_with(|| left.cmp(right))
}

/// 폴더 먼저, 그다음 `compare_names` 순으로 정렬한다.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|left, right| right.is_dir.cmp(&left.is_dir).then_with(|| compare_names(&left.name, &right.name)));
}

/// 폴더 key가 파일 시스템 루트(`/`, `C:/`, 배포판·서버 공유의 맨 위)인지.
fn is_file_system_root(key: &str) -> bool {
    let bytes = key.as_bytes();
    let is_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if let Some(share) = key.strip_prefix("\\\\") {
        share.split('\\').filter(|part| !part.is_empty()).count() <= 2
    } else if is_drive {
        key.len() <= 3
    } else {
        key == "/"
    }
}

/// 폴더 key가 WSL 배포판의 홈 폴더(`\\wsl.localhost\<배포판>\home\<사용자>`·`\\wsl$\…`, 또는 `…\root`)인지.
///
/// WSL은 홈 위치를 묻지 않고 경로 모양만으로 판정한다(데몬·`wsl.exe`를 부르지 않는다).
/// 사용자가 홈을 `/home/<사용자>`나 `/root`가 아닌 곳으로 바꿨으면 알아보지 못한다.
fn is_wsl_home(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let parts: Vec<&str> = lower
        .strip_prefix("\\\\")
        .map_or(Vec::new(), |rest| rest.split('\\').filter(|part| !part.is_empty()).collect());
    let is_share = parts.first().is_some_and(|host| *host == "wsl.localhost" || *host == "wsl$");
    is_share && matches!(parts.get(2..), Some(["home", _]) | Some(["root"]))
}

/// 파일 구획이 보여 줄 폴더. 보이는 작업 폴더, 없으면 선택한 터미널의 현재 폴더, 그것도 없으면 묶음 시작 터미널의 현재 폴더다.
///
/// 홈 폴더(WSL 배포판의 홈 포함)와 파일 시스템 루트는 프로젝트가 아니라서 첫 후보가 그것이면 다음 후보로 넘어가지 않고 None이다.
///
/// Examples
/// --------
/// | 작업 폴더    | 선택 터미널    | 시작 터미널 | 결과           |
/// |--------------|----------------|-------------|----------------|
/// | `/w/shop`    | `/w/shop/src`  | -           | `/w/shop`      |
/// | (없음)       | `/tmp/a/`      | `/tmp/b`    | `/tmp/a`       |
/// | (없음)       | (모름)         | `/tmp/b`    | `/tmp/b`       |
/// | (없음)       | 홈 폴더        | `/tmp/b`    | None           |
/// | (없음)       | `/`·`C:\`      | -           | None           |
pub fn files_folder(
    shown_folder: Option<&str>,
    selected_terminal_cwd: Option<&str>,
    group_root_cwd: Option<&str>,
    home: Option<&str>,
) -> Option<String> {
    let first = shown_folder.or(selected_terminal_cwd).or(group_root_cwd);
    first
        .map(folder_key)
        .filter(|key| !key.is_empty())
        .filter(|key| !home.is_some_and(|home| same_folder(home, key)))
        .filter(|key| !is_file_system_root(key))
        .filter(|key| !is_wsl_home(key))
}

/// 머리줄의 새 파일·새 폴더가 만들 폴더. 마지막으로 펼친 폴더가 아직 펼쳐져 있고 목록 폴더 안이면 그 폴더, 아니면 목록 폴더 맨 위다.
///
/// 폴더를 펼친 사용자는 그 안에서 일하는 중이다. 만드는 위치는 새 항목 창의 제목에 보이므로 착오가 있어도 확인할 수 있다.
///
/// Examples
/// --------
/// | 마지막으로 펼침 | 지금 펼친 폴더 | 목록 폴더 | 결과        |
/// |-----------------|----------------|-----------|-------------|
/// | `/p/src`        | `/p/src`       | `/p`      | `/p/src`    |
/// | `/p/src`        | (없음)         | `/p`      | `/p`        |
/// | `/q/src`        | `/q/src`       | `/p`      | `/p`        |
pub fn creation_folder(
    last_expanded: Option<&str>,
    expanded: &HashSet<String>,
    files_folder: &str,
) -> String {
    let inside = last_expanded
        .filter(|last| expanded.contains(*last) && is_inside(files_folder, last) && !same_folder(files_folder, last));
    inside.unwrap_or(files_folder).to_string()
}

/// `parent` 폴더 안 `name` 항목의 경로. 부모가 쓰는 구분자(역슬래시가 있으면 `\`, 아니면 `/`)를 그대로 쓴다.
pub fn child_path(
    parent: &str,
    name: &str,
) -> String {
    let separator = if parent.contains('\\') { '\\' } else { '/' };
    if parent.ends_with(['/', '\\']) { format!("{parent}{name}") } else { format!("{parent}{separator}{name}") }
}

/// 폴더를 작업 폴더 기준 짧은 이름으로 쓴다. 작업 폴더 밖이면 홈 기준 경로이고, 홈 밖이면 경로 그대로다.
///
/// Examples
/// --------
/// | 경로                         | 결과                  |
/// |------------------------------|-----------------------|
/// | `/work/shop-api`             | `shop-api`            |
/// | `/work/shop-api/src/`        | `shop-api/src`        |
/// | `/work/shop/lib` (이름 `Shop`) | `Shop/lib`          |
/// | `/home/me/notes` (홈 `/home/me`) | `~/notes`         |
/// | `/tmp/x`                     | `/tmp/x`              |
pub fn folder_label(
    folders: &[Folder],
    path: &str,
    home: Option<&str>,
) -> String {
    let path = folder_key(path);
    let own = folders.iter().find(|folder| same_folder(&folder.key, &path));
    let containing =
        folders.iter().filter(|folder| is_inside(&folder.key, &path)).max_by_key(|folder| folder.key.len());
    let (base_key, base_label) = match own.or(containing) {
        Some(folder) => (Some(folder.key.as_str()), folder.label.as_deref()),
        None => (home.filter(|home| is_inside(home, &path)), None),
    };
    match base_key {
        Some(key) => {
            let relative = path.get(folder_key(key).len()..).unwrap_or("").trim_matches(['/', '\\']);
            let name = folder_name(key, base_label, home);
            if relative.is_empty() { name } else { format!("{name}/{}", relative.replace('\\', "/")) }
        }
        None => path,
    }
}

/// 읽은 폴더를 화면 행으로 펴 놓는다. 펼친 폴더의 안쪽 항목은 그 폴더 바로 아래에 한 단계 깊게 놓고,
/// 비었거나 읽지 못한 폴더(맨 위 포함)에는 안내 행 하나를 놓는다.
pub fn rows(directory: &Directory) -> Vec<FileRow> {
    let mut out = Vec::new();
    push_rows(directory, "", 0, &mut out);
    out
}

fn push_rows(
    directory: &Directory,
    path: &str,
    depth: usize,
    out: &mut Vec<FileRow>,
) {
    let notice = if !directory.found {
        Some(NOT_FOUND_NOTICE)
    } else if directory.empty {
        Some(EMPTY_NOTICE)
    } else {
        None
    };
    if let Some(notice) = notice {
        out.push(FileRow {
            path: path.to_string(),
            name: String::new(),
            depth,
            is_dir: false,
            expanded: false,
            notice: Some(notice),
        });
    }
    for entry in &directory.children {
        out.push(FileRow {
            path: entry.path.clone(),
            name: entry.name.clone(),
            depth,
            is_dir: entry.is_dir,
            expanded: entry.expanded,
            notice: None,
        });
        if let (true, Some(inner)) = (entry.expanded, entry.directory.as_deref()) {
            push_rows(inner, &entry.path, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::settings_text::SettingValues;
    use crate::work_model::Folder;

    fn hidden(native: &str) -> Vec<String> {
        let mut names: Vec<String> = hidden_names(&SettingValues::from_texts(native, "")).into_iter().collect();
        names.sort();
        names
    }

    const DEFAULT_NAMES: [&str; 7] = [".DS_Store", ".git", ".hg", ".svn", "CVS", "Thumbs.db", "__pycache__"];

    #[test]
    fn test_c_files_1_without_settings_the_seven_default_names_are_hidden() {
        assert_eq!(hidden(""), DEFAULT_NAMES);
    }

    #[test]
    fn test_c_files_2_the_every_subfolder_prefix_is_stripped_and_plain_names_are_added() {
        let names = hidden(r#"{"files.exclude": {"**/node_modules": true, "dist": true}}"#);
        assert!(names.contains(&"node_modules".to_string()), "{names:?}");
        assert!(names.contains(&"dist".to_string()), "{names:?}");
        assert_eq!(names.len(), 9, "the defaults stay: {names:?}");
    }

    #[test]
    fn test_c_files_3_patterns_with_wildcards_or_paths_are_ignored() {
        let text = r#"{"files.exclude": {
            "**/*.pyc": true, "*.log": true, "src/gen": true, "**/{a,b}": true, "[x]y": true, "?z": true
        }}"#;
        assert_eq!(hidden(text), DEFAULT_NAMES);
    }

    #[test]
    fn test_c_files_4_turned_off_patterns_show_the_name_again() {
        let names = hidden(r#"{"files.exclude": {"**/.git": false, "**/__pycache__": false, "**/out": false}}"#);
        assert_eq!(names, [".DS_Store", ".hg", ".svn", "CVS", "Thumbs.db"]);
    }

    fn entry(
        name: &str,
        is_dir: bool,
    ) -> Entry {
        Entry { path: format!("/p/{name}"), name: name.to_string(), is_dir, expanded: false, directory: None }
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn test_c_files_5_folders_come_first_and_names_compare_without_case() {
        let mut entries = vec![
            entry("b.txt", false),
            entry("README.md", false),
            entry("src", true),
            entry("Zed", true),
            entry("a.js", false),
        ];
        sort_entries(&mut entries);
        assert_eq!(names(&entries), ["src", "Zed", "a.js", "b.txt", "README.md"]);
    }

    #[test]
    fn test_c_files_6_names_follow_the_old_apps_locale_order_for_punctuation_digits_and_case_ties() {
        // 기존 앱(`localeCompare`)이 낸 순서를 그대로 고정한 표다(ASCII 이름).
        let expected = [
            "_docs",
            "_x",
            "-x",
            ".github",
            "(x)",
            "[x]",
            "@x",
            "#x",
            "+x",
            "~",
            "~x",
            "$x",
            "1a",
            "a b",
            "a_b",
            "a-b",
            "a.b",
            "a.txt",
            "A.txt",
            "a1",
            "a10",
            "a2",
            "ab",
            "aB",
            "Ab",
            "AB",
            "b.txt",
            "B.txt",
            "Cargo.lock",
            "Cargo.toml",
            "eb",
            "file_1.txt",
            "file-1.txt",
            "file.1.txt",
            "file1.txt",
            "makefile",
            "Makefile",
            "node_modules",
            "README.md",
            "src",
            "Zed",
            "zeta.txt",
        ];
        let mut entries: Vec<Entry> = expected.iter().rev().map(|name| entry(name, false)).collect();
        sort_entries(&mut entries);
        assert_eq!(names(&entries), expected);
        let mut folders: Vec<Entry> =
            [".github", "_x", "a.txt", "A.txt"].iter().map(|name| entry(name, name.len() < 8)).collect();
        sort_entries(&mut folders);
        assert_eq!(names(&folders), ["_x", ".github", "a.txt", "A.txt"]);
    }

    #[test]
    fn test_c_files_7_the_shown_work_folder_wins_over_terminal_folders() {
        let found = files_folder(Some("/work/shop"), Some("/work/shop/src"), Some("/other"), Some("/home/me"));
        assert_eq!(found.as_deref(), Some("/work/shop"));
    }

    #[test]
    fn test_c_files_8_without_a_work_folder_the_selected_terminal_then_the_group_start_terminal_decide() {
        assert_eq!(
            files_folder(None, Some("/tmp/a/"), Some("/tmp/b"), Some("/home/me")).as_deref(),
            Some("/tmp/a"),
            "the selected terminal's folder, without the trailing separator"
        );
        assert_eq!(files_folder(None, None, Some("/tmp/b"), Some("/home/me")).as_deref(), Some("/tmp/b"));
        assert_eq!(files_folder(None, None, None, Some("/home/me")), None);
    }

    #[test]
    fn test_c_files_9_the_home_folder_is_not_listed_even_when_spelled_differently() {
        assert_eq!(files_folder(None, Some("/home/me/"), None, Some("/home/me")), None);
        assert_eq!(files_folder(Some("C:/Users/Me"), None, None, Some(r"c:\users\me\")), None);
        assert_eq!(files_folder(Some("/home/me/proj"), None, None, Some("/home/me")).as_deref(), Some("/home/me/proj"));
        // 첫 후보가 홈이면 다음 후보로 넘어가지 않고 목록을 숨긴다.
        assert_eq!(files_folder(None, Some("/home/me"), Some("/tmp/b"), Some("/home/me")), None);
    }

    #[test]
    fn test_c_files_10_file_system_roots_are_not_listed() {
        for root in ["/", r"C:\", "C:/", "c:", r"\\wsl.localhost\Ubuntu", r"\\wsl$\Ubuntu\"] {
            assert_eq!(files_folder(None, Some(root), None, Some("/home/me")), None, "{root}");
        }
        assert!(files_folder(None, Some(r"C:\Users"), None, None).is_some());
        assert!(files_folder(None, Some(r"\\wsl.localhost\Ubuntu\home"), None, None).is_some());
    }

    #[test]
    fn test_c_files_32_the_home_folder_of_a_wsl_distribution_is_not_listed() {
        for home in [
            r"\\wsl.localhost\Ubuntu\home\me",
            r"\\wsl.localhost\Ubuntu\home\me\",
            "//wsl.localhost/Ubuntu/home/me",
            r"\\WSL$\Debian\home\Me",
            r"\\wsl$\Ubuntu\root",
            r"\\wsl.localhost\Ubuntu-24.04\root\",
        ] {
            assert_eq!(files_folder(None, Some(home), None, None), None, "{home}");
        }
        for project in [
            r"\\wsl.localhost\Ubuntu\home\me\proj",
            r"\\wsl.localhost\Ubuntu\home",
            r"\\wsl.localhost\Ubuntu\root\proj",
            r"\\wsl.localhost\Ubuntu\tmp\x",
            r"\\server\share\home\me",
        ] {
            assert!(files_folder(None, Some(project), None, None).is_some(), "{project}");
        }
    }

    fn set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn test_c_files_11_new_entries_go_into_the_last_expanded_folder_while_it_is_open_and_inside() {
        let expanded = set(&["/p/src", "/p/src/deep"]);
        assert_eq!(creation_folder(Some("/p/src/deep"), &expanded, "/p"), "/p/src/deep");
        assert_eq!(creation_folder(Some("/p/src"), &expanded, "/p"), "/p/src");
    }

    #[test]
    fn test_c_files_12_otherwise_new_entries_go_to_the_top_of_the_listed_folder() {
        let expanded = set(&["/p/src"]);
        assert_eq!(creation_folder(None, &expanded, "/p"), "/p");
        assert_eq!(creation_folder(Some("/p/lib"), &expanded, "/p"), "/p", "collapsed since");
        assert_eq!(creation_folder(Some("/q/src"), &set(&["/q/src"]), "/p"), "/p", "outside the listed folder");
        assert_eq!(creation_folder(Some("/p"), &set(&["/p"]), "/p"), "/p");
        assert_eq!(creation_folder(Some("/pp/x"), &set(&["/pp/x"]), "/p"), "/p", "a sibling with the same prefix");
    }

    #[test]
    fn test_c_files_13_child_paths_keep_the_separator_style_of_the_parent() {
        assert_eq!(child_path("/p/src", "a.js"), "/p/src/a.js");
        assert_eq!(child_path("C:/Users/me", "a.js"), "C:/Users/me/a.js");
        assert_eq!(child_path(r"\\wsl.localhost\Ubuntu\home", "a.js"), r"\\wsl.localhost\Ubuntu\home\a.js");
        assert_eq!(child_path("/", "etc"), "/etc");
        assert_eq!(child_path(r"C:\", "x"), r"C:\x");
    }

    fn folder(key: &str) -> Folder {
        Folder { key: key.to_string(), expanded: true, label: None }
    }

    #[test]
    fn test_c_files_14_folder_labels_are_short_names_under_the_work_folder_or_home_paths_outside() {
        let folders = [folder("/work/shop-api"), Folder { label: Some("Shop".to_string()), ..folder("/work/shop") }];
        let label = |path: &str| folder_label(&folders, path, Some("/home/me"));
        assert_eq!(label("/work/shop-api"), "shop-api");
        assert_eq!(label("/work/shop-api/src"), "shop-api/src");
        assert_eq!(label("/work/shop-api/src/deep/"), "shop-api/src/deep");
        assert_eq!(label("/work/shop/lib"), "Shop/lib", "the renamed list label");
        assert_eq!(label("/home/me/notes/2026"), "~/notes/2026");
        assert_eq!(label("/tmp/x"), "/tmp/x");
    }

    fn dir(children: Vec<Entry>) -> Directory {
        Directory { found: true, empty: children.is_empty(), children }
    }

    #[test]
    fn test_c_files_15_rows_list_open_folders_in_place_and_notices_for_empty_or_missing_ones() {
        let inner = dir(vec![entry("x.rs", false)]);
        let empty = dir(Vec::new());
        let mut src = entry("src", true);
        src.expanded = true;
        src.directory = Some(Box::new(inner));
        let mut docs = entry("docs", true);
        docs.expanded = true;
        docs.directory = Some(Box::new(empty));
        let top = dir(vec![src, docs, entry("README.md", false)]);
        let all_rows = rows(&top);
        let listed: Vec<(usize, &str, bool, Option<&str>)> =
            all_rows.iter().map(|row| (row.depth, row.name.as_str(), row.is_dir, row.notice)).collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                (0, "src", true, None),
                (1, "x.rs", false, None),
                (0, "docs", true, None),
                (1, "", false, Some(EMPTY_NOTICE)),
                (0, "README.md", false, None),
            ]
        );
        let notice_of = |directory: &Directory| rows(directory).first().and_then(|row| row.notice);
        let missing = Directory { found: false, empty: true, children: Vec::new() };
        assert_eq!(notice_of(&missing), Some(NOT_FOUND_NOTICE));
        assert_eq!(notice_of(&dir(Vec::new())), Some(EMPTY_NOTICE));
    }
}
