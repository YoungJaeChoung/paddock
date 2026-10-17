//! 파일 목록의 Git 변경 표시: 바뀐 파일에 상태 글자와 색을, 바뀐 파일을 품은 폴더에 색을 붙인다.
//!
//! 변경 목록(`git_status::Change`)에서 파일·폴더 표시를 찾는 `FileMarks`를 만든다. Source control 보기로 가지 않아도 파일 목록에서
//! 무엇이 바뀌었는지 보이게 하는 것이 목적이다.
use std::collections::HashMap;

use crate::file_tree::child_path;
use crate::git_status::{Change, Letter, Section, mark_of};
use crate::theme::Colors;
use crate::work_model::folder_identity;

/// 표시 색의 종류. 실제 색은 `theme::Colors.git_*`가 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkColor {
    Modified,
    Untracked,
    Added,
    Deleted,
    Renamed,
    Conflicting,
}

impl MarkColor {
    /// 폴더 하나에 여러 변경이 섞일 때 앞선 것이 이긴다: 충돌 > 수정 > 삭제 > 이름 바꿈 > 추가 > 추적 안 함. 작을수록 급하다.
    fn urgency(self) -> u8 {
        match self {
            Self::Conflicting => 0,
            Self::Modified => 1,
            Self::Deleted => 2,
            Self::Renamed => 3,
            Self::Added => 4,
            Self::Untracked => 5,
        }
    }

    /// 상태 글자의 색.
    pub fn of_letter(letter: Letter) -> Self {
        match letter {
            Letter::Modified => Self::Modified,
            Letter::Untracked => Self::Untracked,
            Letter::Added => Self::Added,
            Letter::Deleted => Self::Deleted,
            Letter::Renamed => Self::Renamed,
            Letter::Conflict | Letter::Copied => Self::Conflicting,
            Letter::Typechange => Self::Modified,
        }
    }
}

/// 변경 행의 표시 색. 병합 충돌 행은 글자가 `D`여도 늘 충돌 색이고, 나머지는 글자의 색이다.
pub fn color_of(change: &Change) -> MarkColor {
    match change.section {
        Section::Merge => MarkColor::Conflicting,
        _ => MarkColor::of_letter(mark_of(change)),
    }
}

/// 표시 색의 실제 색(0xRRGGBB).
pub fn git_color(
    colors: &Colors,
    color: MarkColor,
) -> u32 {
    match color {
        MarkColor::Modified => colors.git_modified,
        MarkColor::Untracked => colors.git_untracked,
        MarkColor::Added => colors.git_added,
        MarkColor::Deleted => colors.git_deleted,
        MarkColor::Renamed => colors.git_renamed,
        MarkColor::Conflicting => colors.git_conflicting,
    }
}

/// 파일 하나의 표시.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub letter: Letter,
    pub color: MarkColor,
}

/// 파일·폴더 표시 조회표.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMarks {
    files: HashMap<String, Mark>,
    folders: HashMap<String, MarkColor>,
}

/// 구획이 앞설수록 먼저 온 것으로 본다: 병합 → 스테이징 → 변경 → 추적 안 함.
fn section_order(section: Section) -> u8 {
    section as u8
}

impl FileMarks {
    /// `root` 저장소의 변경 목록으로 만든다. 같은 파일이 여러 구획에 있으면 앞선 구획의 것을 쓴다.
    pub fn new(
        root: &str,
        changes: &[Change],
    ) -> Self {
        let mut ordered: Vec<&Change> = changes.iter().collect();
        ordered.sort_by_key(|change| section_order(change.section));
        let mut marks = Self::default();
        for change in ordered {
            let key = folder_identity(&child_path(root, &change.path));
            marks.files.entry(key).or_insert(Mark { letter: mark_of(change), color: color_of(change) });
        }
        // 파일의 모든 조상 폴더에 색을 올린다. 더 급한 색이 이미 있으면 그대로 둔다.
        for (key, mark) in &marks.files {
            let mut parent = key.as_str();
            while let Some(end) = parent.rfind(['/', '\\']) {
                parent = &parent[..end];
                if parent.is_empty() || parent.ends_with(':') {
                    break;
                }
                let entry = marks.folders.entry(parent.to_string()).or_insert(mark.color);
                if mark.color.urgency() < entry.urgency() {
                    *entry = mark.color;
                }
            }
        }
        marks
    }

    /// 파일의 표시. 바뀌지 않았으면 None이다.
    pub fn file(
        &self,
        path: &str,
    ) -> Option<Mark> {
        self.files.get(&folder_identity(path)).copied()
    }

    /// 폴더의 색. 그 아래(하위 폴더 포함)에 변경이 있을 때만 있다.
    pub fn folder(
        &self,
        path: &str,
    ) -> Option<MarkColor> {
        self.folders.get(&folder_identity(path)).copied()
    }

    /// 표시할 변경이 하나도 없다.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        path: &str,
        index: char,
        worktree: char,
        section: Section,
    ) -> Change {
        Change { path: path.into(), orig_path: None, index, worktree, section }
    }

    #[test]
    fn test_c_marks_1_a_changed_file_gets_its_letter_and_color_and_its_ancestors_get_the_color() {
        let marks = FileMarks::new("/r", &[change("src/a.js", '.', 'M', Section::Changes)]);
        assert_eq!(marks.file("/r/src/a.js"), Some(Mark { letter: Letter::Modified, color: MarkColor::Modified }));
        assert_eq!(marks.folder("/r/src"), Some(MarkColor::Modified));
        assert_eq!(marks.folder("/r/src/"), Some(MarkColor::Modified), "a trailing separator is ignored");
        assert_eq!(marks.folder("/r"), Some(MarkColor::Modified));
    }

    #[test]
    fn test_c_marks_2_a_folder_whose_name_only_starts_the_same_and_unchanged_files_get_nothing() {
        let marks = FileMarks::new("/r", &[change("src/a.js", '.', 'M', Section::Changes)]);
        assert_eq!(marks.folder("/r/s"), None);
        assert_eq!(marks.folder("/r/src2"), None);
        assert_eq!(marks.file("/r/src/b.js"), None);
        assert!(FileMarks::new("/r", &[]).folder("/r").is_none());
    }

    #[test]
    fn test_c_marks_3_mixed_changes_in_one_folder_use_the_most_urgent_color() {
        let marks = FileMarks::new(
            "/r",
            &[
                change("a/new.js", '?', '?', Section::Untracked),
                change("a/old.js", '.', 'M', Section::Changes),
                change("b/x.js", '.', 'M', Section::Changes),
                change("b/y.js", 'U', 'U', Section::Merge),
            ],
        );
        assert_eq!(marks.folder("/r/a"), Some(MarkColor::Modified));
        assert_eq!(marks.folder("/r/b"), Some(MarkColor::Conflicting));
        assert_eq!(marks.folder("/r"), Some(MarkColor::Conflicting));
    }

    #[test]
    fn test_c_marks_4_the_same_file_in_two_sections_uses_the_earlier_section() {
        let marks = FileMarks::new(
            "/r",
            &[change("a.js", 'M', 'M', Section::Changes), change("a.js", 'A', 'M', Section::Staged)],
        );
        assert_eq!(marks.file("/r/a.js").map(|mark| mark.letter), Some(Letter::Added), "staged comes before changes");
    }

    #[test]
    fn test_c_marks_6_colors_follow_the_extension_rules_conflicts_are_always_the_conflict_color() {
        let color = |section, index, worktree| {
            let change = Change { path: "p".into(), orig_path: None, index, worktree, section };
            color_of(&change)
        };
        assert_eq!(color(Section::Merge, 'D', 'U'), MarkColor::Conflicting, "deleted by them is still a conflict");
        assert_eq!(color(Section::Merge, 'U', 'U'), MarkColor::Conflicting);
        assert_eq!(color(Section::Staged, 'C', '.'), MarkColor::Conflicting, "copied uses the letter rule of the list");
        assert_eq!(color(Section::Staged, 'T', '.'), MarkColor::Modified);
        assert_eq!(color(Section::Changes, '.', 'D'), MarkColor::Deleted);
        assert_eq!(color(Section::Staged, 'A', '.'), MarkColor::Added);
        assert_eq!(color(Section::Untracked, '?', '?'), MarkColor::Untracked);
        let marks = FileMarks::new(
            "/r",
            &[Change { path: "d.txt".into(), orig_path: None, index: 'D', worktree: 'U', section: Section::Merge }],
        );
        assert_eq!(marks.file("/r/d.txt"), Some(Mark { letter: Letter::Deleted, color: MarkColor::Conflicting }));
        assert_eq!(marks.folder("/r"), Some(MarkColor::Conflicting));
    }

    #[test]
    fn test_c_marks_5_windows_and_wsl_paths_compare_by_the_same_identity_as_work_folders() {
        let marks = FileMarks::new("c:\\Users\\Me\\Repo", &[change("src/a.rs", '.', 'M', Section::Changes)]);
        assert!(marks.file("C:/users/me/repo/src/a.rs").is_some());
        assert!(marks.folder("C:/Users/Me/Repo/src").is_some());
        let wsl = FileMarks::new(
            r"\\wsl.localhost\Ubuntu\home\me\r",
            &[change("a b/한글.txt", '?', '?', Section::Untracked)],
        );
        assert!(wsl.file(r"\\WSL.localhost\ubuntu\home\me\r\a b\한글.txt").is_some());
        assert_eq!(wsl.folder(r"\\wsl.localhost\Ubuntu\home\me\r\a b"), Some(MarkColor::Untracked));
    }
}
