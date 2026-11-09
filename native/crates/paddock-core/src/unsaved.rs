//! 고치고 아직 저장하지 않은 파일: 어느 파일이 그런지 쥐는 집합과 저장·종료·디스크 변경을 묻는 문구.
//!
//! 화면을 모르는 순수 규칙이다. 편집기 글자 자체는 앱이 쥐고, 여기서는 "저장하지 않은 것이 있는가"만 센다.
use std::collections::BTreeSet;

use crate::paths::rebased;

/// 저장하지 않은 파일의 경로 집합. 고치기 시작하면 넣고 저장하거나 버리면 뺀다. undo로 글자가 같아져도 저장하기 전에는 빼지 않는다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnsavedSet {
    paths: BTreeSet<String>,
}

impl UnsavedSet {
    pub fn mark(
        &mut self,
        path: &str,
    ) {
        self.paths.insert(path.to_string());
    }

    pub fn clear(
        &mut self,
        path: &str,
    ) {
        self.paths.remove(path);
    }

    pub fn contains(
        &self,
        path: &str,
    ) -> bool {
        self.paths.contains(path)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// `keep`이 참인 경로만 남긴다(닫힌 탭의 경로를 치울 때 쓴다).
    pub fn retain(
        &mut self,
        mut keep: impl FnMut(&str) -> bool,
    ) {
        self.paths.retain(|path| keep(path));
    }

    /// 저장하지 않은 경로(정렬된 순서).
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.paths.iter()
    }

    /// `folder`(또는 같은 이름의 파일) 안에서 저장하지 않은 파일의 개수.
    ///
    /// Examples
    /// --------
    /// | 저장 안 한 파일        | folder  | 결과 |
    /// |------------------------|---------|------|
    /// | `/w/a.txt`, `/w/s/b.txt` | `/w`  | 2    |
    /// | `/w/a.txt`, `/w/s/b.txt` | `/w/s`| 1    |
    /// | `/w/a.txt`             | `/x`    | 0    |
    pub fn count_inside(
        &self,
        folder: &str,
    ) -> usize {
        self.paths.iter().filter(|path| rebased(folder, folder, path).is_some()).count()
    }
}

/// 파일 탭을 닫을 때 저장 여부를 묻는 창의 제목.
pub fn save_changes_title(name: &str) -> String {
    format!("Save changes to {name}?")
}

/// 창을 닫거나 종료할 때 저장하지 않은 파일이 여럿임을 알리는 문장. 하나일 때는 `save_changes_title`을 쓴다.
///
/// Examples
/// --------
/// | n_files | 결과                              |
/// |---------|-----------------------------------|
/// | 1       | `1 file has unsaved changes.`     |
/// | 3       | `3 files have unsaved changes.`   |
pub fn unsaved_files_text(n_files: usize) -> String {
    if n_files == 1 {
        "1 file has unsaved changes.".to_string()
    } else {
        format!("{n_files} files have unsaved changes.")
    }
}

/// 묶음·폴더를 닫는 확인 문장에 덧붙이는 표시. 저장하지 않은 파일이 없으면 빈 글자다.
///
/// Examples
/// --------
/// | n_files | 결과           |
/// |---------|----------------|
/// | 0       | ``             |
/// | 2       | ` (2 unsaved)` |
pub fn unsaved_suffix(n_files: usize) -> String {
    if n_files == 0 { String::new() } else { format!(" ({n_files} unsaved)") }
}

/// 저장 여부를 묻는 창(파일 탭 닫기·종료) 설명.
pub const LOSE_CHANGES_DETAIL: &str = "Your changes will be lost if you don't save them.";

/// 열린 파일이 디스크에서 바뀌었음을 알리는 띠의 글자.
pub const DISK_CHANGED_BAR: &str = "This file changed on disk.";

/// 디스크에서 바뀐 파일을 저장하려 할 때 덮어써도 되는지 묻는 문장.
pub fn overwrite_prompt(name: &str) -> String {
    format!("{name} changed on disk since you opened it. Overwrite?")
}

/// 저장하지 못했을 때 상태 줄에 보일 문장. `reason`은 입출력 오류를 평문으로 만든 조각이다.
pub fn save_failed_text(
    name: &str,
    reason: &str,
) -> String {
    format!("Could not save {name}: {reason}")
}

/// `Save all`로 종료하려는데 저장하는 동안 더 고친 파일이 남았음을 알리는 문장(묻기 창을 다시 띄울 때 상태 줄에 둔다).
///
/// Examples
/// --------
/// | n_files | 결과                            |
/// |---------|---------------------------------|
/// | 1       | `1 file changed while saving.`  |
/// | 3       | `3 files changed while saving.` |
pub fn changed_while_saving_text(n_files: usize) -> String {
    if n_files == 1 {
        "1 file changed while saving.".to_string()
    } else {
        format!("{n_files} files changed while saving.")
    }
}

/// `Save all`이 저장 명령을 하나도 보내지 못했을 때(저장할 수 없는 파일뿐) 상태 줄에 둘 문장.
///
/// Examples
/// --------
/// | n_files | 결과                                  |
/// |---------|---------------------------------------|
/// | 1       | `1 file could not be saved right now.`  |
/// | 2       | `2 files could not be saved right now.` |
pub fn quit_save_blocked_text(n_files: usize) -> String {
    if n_files == 1 {
        "1 file could not be saved right now.".to_string()
    } else {
        format!("{n_files} files could not be saved right now.")
    }
}

/// 상태 줄 오른쪽 저장 표시의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStatus {
    /// 저장하지 않은 변경이 있다(`Unsaved changes` + `Save` 단추).
    Unsaved,
    /// 방금 저장했다(`Saved`, 2초).
    Saved,
}

/// 지금 파일 탭의 저장 표시. 저장하지 않은 변경이 있으면 그것이 먼저다(방금 저장한 뒤 더 고친 경우).
///
/// Examples
/// --------
/// | unsaved | saved_recently | 결과      |
/// |---------|----------------|-----------|
/// | true    | 어느 쪽이든    | `Unsaved` |
/// | false   | true           | `Saved`   |
/// | false   | false          | None      |
pub fn save_status(
    unsaved: bool,
    saved_recently: bool,
) -> Option<SaveStatus> {
    if unsaved {
        Some(SaveStatus::Unsaved)
    } else if saved_recently {
        Some(SaveStatus::Saved)
    } else {
        None
    }
}

/// 터미널·묶음·폴더를 닫기 전에 묻는가: 살아 있는 셸이 있거나 함께 닫히는 파일에 저장하지 않은 변경이 있으면 묻는다(터미널이 모두 끝났어도 편집은 소리 없이 사라지면 안 된다).
///
/// Examples
/// --------
/// | 살아 있는 셸 | 저장 안 한 파일 | 결과  |
/// |--------------|-----------------|-------|
/// | 있음         | 0               | true  |
/// | 없음         | 0               | false |
/// | 없음         | 2               | true  |
pub fn needs_close_prompt(
    any_shell_alive: bool,
    n_unsaved: usize,
) -> bool {
    any_shell_alive || n_unsaved > 0
}

/// `Don't Save`로 탭을 닫았는데 저장 명령이 이미 가 있어 그 파일이 저장됐을 때 알리는 문장.
pub fn saved_before_close_text(name: &str) -> String {
    format!("{name} was saved before the tab closed.")
}

/// 이름을 바꾸는 중인 항목 안의 파일을 저장하려 할 때 알리는 문장(옛 경로에 파일이 다시 생기지 않게 저장을 미룬다).
pub const RENAME_PENDING_TEXT: &str = "Wait for the rename to finish, then save again.";

/// 저장을 시도한 뒤에도 그 파일이 저장하지 않은 상태로 남는가: 쓰지 못했거나, 쓰는 동안 더 고쳤으면 남는다.
///
/// Examples
/// --------
/// | written | edited_meanwhile | 결과  |
/// |---------|------------------|-------|
/// | true    | false            | false |
/// | true    | true             | true  |
/// | false   | false            | true  |
pub fn stays_unsaved_after_save(
    written: bool,
    edited_meanwhile: bool,
) -> bool {
    !written || edited_meanwhile
}

/// 디스크의 파일이 바뀐 것을 알았을 때 열린 파일을 어떻게 할지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskChange {
    /// 고친 내용이 없다: 디스크 내용을 다시 읽는다.
    Reload,
    /// 고친 내용이 있고 디스크가 열었을 때와 다르다: 다시 읽지 않고 `Reload`·`Keep mine`을 묻는다.
    Ask,
    /// 고친 내용이 있고 디스크는 열었을 때 그대로다: 아무것도 하지 않는다.
    Keep,
}

/// 열린 파일을 디스크에서 다시 읽으려 할 때, 고친 내용(`unsaved`)이 있으면 덮지 않고 디스크가 열었을 때(`known`)와 달라졌을 때만(`now`) 묻는다.
///
/// Examples
/// --------
/// | unsaved | known | now | 결과     |
/// |---------|-------|-----|----------|
/// | false   | 1     | 2   | `Reload` |
/// | true    | 1     | 2   | `Ask`    |
/// | true    | 1     | 1   | `Keep`   |
pub fn disk_change_effect<T: PartialEq>(
    unsaved: bool,
    known: &T,
    now: &T,
) -> DiskChange {
    if !unsaved {
        DiskChange::Reload
    } else if known != now {
        DiskChange::Ask
    } else {
        DiskChange::Keep
    }
}

/// 상태 줄 저장 표시: 저장하지 않은 변경이 있다.
pub const STATUS_UNSAVED: &str = "Unsaved changes";

/// 상태 줄 저장 표시: 방금 저장했다.
pub const STATUS_SAVED: &str = "Saved";

/// 저장 직후 `STATUS_SAVED`를 보이는 시간(초).
pub const SAVED_SHOWN_SECONDS: u64 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_edit_6_unsaved_set_marks_clears_and_counts_inside_a_folder() {
        let mut set = UnsavedSet::default();
        assert!(set.is_empty());
        set.mark("/w/a.txt");
        set.mark("/w/s/b.txt");
        set.mark("/w/a.txt");
        assert_eq!(set.len(), 2);
        assert_eq!(set.count_inside("/w"), 2);
        assert_eq!(set.count_inside("/w/s"), 1);
        assert_eq!(set.count_inside("/x"), 0);
        assert_eq!(set.count_inside("/w/a.txt"), 1);
        set.clear("/w/a.txt");
        assert!(!set.contains("/w/a.txt"));
        assert_eq!(set.paths().cloned().collect::<Vec<_>>(), ["/w/s/b.txt"]);
    }

    #[test]
    fn test_c_edit_7_messages_match_the_old_apps_sentences() {
        assert_eq!(save_changes_title("a.txt"), "Save changes to a.txt?");
        assert_eq!(unsaved_files_text(1), "1 file has unsaved changes.");
        assert_eq!(unsaved_files_text(3), "3 files have unsaved changes.");
        assert_eq!(unsaved_suffix(0), "");
        assert_eq!(unsaved_suffix(2), " (2 unsaved)");
        assert_eq!(overwrite_prompt("a.txt"), "a.txt changed on disk since you opened it. Overwrite?");
        assert_eq!(save_failed_text("a.txt", "permission denied."), "Could not save a.txt: permission denied.");
    }

    #[test]
    fn test_c_edit_11_unsaved_wins_over_a_recent_save_in_the_status_line() {
        assert_eq!(save_status(true, true), Some(SaveStatus::Unsaved));
        assert_eq!(save_status(true, false), Some(SaveStatus::Unsaved));
        assert_eq!(save_status(false, true), Some(SaveStatus::Saved));
        assert_eq!(save_status(false, false), None);
    }

    #[test]
    fn test_c_edit_14_close_and_rename_sentences() {
        assert_eq!(saved_before_close_text("a.txt"), "a.txt was saved before the tab closed.");
        assert!(RENAME_PENDING_TEXT.starts_with("Wait for the rename"));
    }

    #[test]
    fn test_c_edit_13_quit_save_sentences_count_files() {
        assert_eq!(changed_while_saving_text(1), "1 file changed while saving.");
        assert_eq!(changed_while_saving_text(3), "3 files changed while saving.");
        assert_eq!(quit_save_blocked_text(1), "1 file could not be saved right now.");
        assert_eq!(quit_save_blocked_text(2), "2 files could not be saved right now.");
    }

    #[test]
    fn test_c_edit_12_closing_asks_when_a_shell_is_alive_or_edits_would_be_lost() {
        assert!(needs_close_prompt(true, 0));
        assert!(!needs_close_prompt(false, 0));
        assert!(needs_close_prompt(false, 2));
    }

    #[test]
    fn test_c_d67_a_failed_or_raced_save_keeps_the_file_unsaved() {
        assert!(!stays_unsaved_after_save(true, false));
        assert!(stays_unsaved_after_save(true, true));
        assert!(stays_unsaved_after_save(false, false));
        assert!(stays_unsaved_after_save(false, true));
    }

    #[test]
    fn test_c_d67_unsaved_text_is_never_replaced_by_a_reread_and_only_a_changed_disk_asks() {
        assert_eq!(disk_change_effect(false, &1, &2), DiskChange::Reload);
        assert_eq!(disk_change_effect(false, &1, &1), DiskChange::Reload);
        assert_eq!(disk_change_effect(true, &1, &2), DiskChange::Ask);
        assert_eq!(disk_change_effect(true, &1, &1), DiskChange::Keep);
    }
}
