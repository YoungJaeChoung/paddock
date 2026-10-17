//! Source control 보기의 순수 규칙: 구획 이름, 화면에 그릴 행 목록, 경로 나누기, 배지 글자, 커밋 단추 상태, 되돌리기 확인 문구.
use std::collections::HashSet;

use paddock_core::git_status::{Change, Section, Status};
use paddock_core::theme::view_badge;

/// 화면 구획. 추적하지 않는 파일은 `Changes`에 함께 보인다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Merge,
    Staged,
    Changes,
}

impl Group {
    /// 구획 머리에 보이는 이름(머리에서 대문자로 바꿔 그린다).
    pub fn title(self) -> &'static str {
        match self {
            Self::Merge => "Merge Changes",
            Self::Staged => "Staged Changes",
            Self::Changes => "Changes",
        }
    }

    /// 상태 글과 대본에서 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Staged => "staged",
            Self::Changes => "changes",
        }
    }

    fn holds(
        self,
        section: Section,
    ) -> bool {
        match self {
            Self::Merge => section == Section::Merge,
            Self::Staged => section == Section::Staged,
            Self::Changes => matches!(section, Section::Changes | Section::Untracked),
        }
    }
}

/// 보이는 구획 순서.
pub const GROUPS: [Group; 3] = [Group::Merge, Group::Staged, Group::Changes];

/// 목록의 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Header { group: Group, n_rows: usize },
    Row { group: Group, change: Change },
}

/// 한 구획의 변경들(`Changes`는 추적 중인 변경 뒤에 추적하지 않는 파일).
pub fn rows_of(
    status: &Status,
    group: Group,
) -> Vec<&Change> {
    let mut rows: Vec<&Change> = status.changes.iter().filter(|change| group.holds(change.section)).collect();
    rows.sort_by_key(|change| change.section);
    rows
}

/// 화면에 그릴 줄 목록: 변경이 있는 구획마다 머리 한 줄과(접히지 않았으면) 변경 행들.
pub fn items(
    status: &Status,
    collapsed: &HashSet<Group>,
) -> Vec<Item> {
    let mut items = Vec::new();
    for group in GROUPS {
        let rows = rows_of(status, group);
        if !rows.is_empty() {
            items.push(Item::Header { group, n_rows: rows.len() });
            if !collapsed.contains(&group) {
                items.extend(rows.into_iter().map(|change| Item::Row { group, change: change.clone() }));
            }
        }
    }
    items
}

/// 구획 머리의 `+`가 스테이징할 경로. None이면 모든 변경을 스테이징해도 된다(`git add -A`).
///
/// `Merge Changes`의 `+`는 충돌 행만 스테이징한다(해결 표시). `Changes`의 `+`는 충돌 행이 없을 때만 전부(`-A`)이고, 충돌 행이 있으면
/// `-A`가 충돌을 해결된 것으로 표시해 버리므로 변경·추적 안 함 행의 경로만 넘긴다.
///
/// Examples
/// --------
/// | 구획      | 충돌 행 | 결과                         |
/// |-----------|---------|------------------------------|
/// | `Merge`   | 있음    | 충돌 행 경로들               |
/// | `Changes` | 없음    | None(전부)                   |
/// | `Changes` | 있음    | 변경·추적 안 함 행 경로들    |
pub fn paths_to_stage(
    status: &Status,
    group: Group,
) -> Option<Vec<String>> {
    let has_merge = status.changes.iter().any(|change| change.section == Section::Merge);
    match (group, has_merge) {
        (Group::Merge, _) | (Group::Changes, true) => {
            Some(rows_of(status, group).into_iter().map(|change| change.path.clone()).collect())
        }
        (Group::Changes, false) | (Group::Staged, _) => None,
    }
}

/// 경로를 (파일 이름, 폴더)로 나눈다. 폴더는 흐리게 보이며 루트 바로 아래 파일은 비어 있다.
///
/// Examples
/// --------
/// | 경로               | 결과                      |
/// |--------------------|---------------------------|
/// | `src/a/b.rs`       | (`b.rs`, `src/a`)         |
/// | `README.md`        | (`README.md`, ``)         |
pub fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (name.to_string(), dir.to_string()),
        None => (path.to_string(), String::new()),
    }
}

/// 보기 줄 배지 글자. 변경이 없으면 배지가 없고, 99를 넘으면 `99+`다.
///
/// Examples
/// --------
/// | 변경 수 | 결과        |
/// |---------|-------------|
/// | 0       | None        |
/// | 18      | `18`        |
/// | 100     | `99+`       |
pub fn badge_text(n_changes: usize) -> Option<String> {
    match n_changes {
        0 => None,
        n if n > view_badge::MAX_COUNT => Some(format!("{}+", view_badge::MAX_COUNT)),
        n => Some(n.to_string()),
    }
}

/// 커밋 단추 자리의 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitState {
    /// 스테이징된 변경이 없다. 단추 대신 안내 글이 보인다.
    NothingStaged,
    /// 메시지가 비어 있다. 단추는 보이지만 누르면 입력을 요구하는 한 줄이 뜬다.
    NeedsMessage,
    Ready,
}

/// 스테이징된 변경 수와 메시지로 커밋 단추 상태를 정한다. 공백뿐인 메시지는 빈 메시지다.
pub fn commit_state(
    n_staged: usize,
    message: &str,
) -> CommitState {
    match (n_staged, message.trim().is_empty()) {
        (0, _) => CommitState::NothingStaged,
        (_, true) => CommitState::NeedsMessage,
        _ => CommitState::Ready,
    }
}

/// 되돌리기 확인 창의 문구.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardPrompt {
    pub title: &'static str,
    pub headline: String,
    pub detail: &'static str,
    pub confirm: &'static str,
}

/// 추적 중인 파일은 `Discard changes in …`, 추적하지 않는 파일은 지우는 일이므로 `Delete untracked file …`로 구분한다.
///
/// Examples
/// --------
/// | 변경                    | 제목              | 머리 문장                         | 단추      |
/// |-------------------------|-------------------|-----------------------------------|-----------|
/// | 추적 중 `src/a.rs`      | Discard changes   | `Discard changes in a.rs?`        | `Discard` |
/// | 추적 안 함 `new.txt`    | Delete untracked file | `Delete untracked file new.txt?` | `Delete`  |
pub fn discard_prompt(change: &Change) -> DiscardPrompt {
    let (name, _) = split_path(&change.path);
    if change.section == Section::Untracked {
        DiscardPrompt {
            title: "Delete untracked file",
            headline: format!("Delete untracked file {name}?"),
            detail: "The file will be deleted from disk. This can't be undone.",
            confirm: "Delete",
        }
    } else {
        DiscardPrompt {
            title: "Discard changes",
            headline: format!("Discard changes in {name}?"),
            detail: "Your changes in this file will be lost. This can't be undone.",
            confirm: "Discard",
        }
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::git_status::parse_porcelain_v2_z;

    use super::*;

    fn status(entries: &str) -> Status {
        parse_porcelain_v2_z(format!("# branch.oid abc\0# branch.head main\0{}", entries.replace(';', "\0")).as_bytes())
    }

    const HEX: &str = "N... 100644 100644 100644 1 2";

    #[test]
    fn test_c_d53_items_group_rows_by_section_with_untracked_files_inside_changes() {
        let status = status(&format!(
            "? new.txt;1 .M {HEX} src/a.rs;1 M. {HEX} s.rs;u UU N... 100644 100644 100644 100644 1 2 3 c.rs;"
        ));
        let items = items(&status, &HashSet::new());
        let shape: Vec<String> = items
            .iter()
            .map(|item| match item {
                Item::Header { group, n_rows } => format!("# {} {n_rows}", group.name()),
                Item::Row { group, change } => format!("{} {}", group.name(), change.path),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "# merge 1",
                "merge c.rs",
                "# staged 1",
                "staged s.rs",
                "# changes 2",
                "changes src/a.rs",
                "changes new.txt",
            ],
            "tracked changes come before untracked files"
        );
    }

    #[test]
    fn test_c_d53_a_collapsed_group_keeps_only_its_header_and_an_empty_group_has_none() {
        let status = status(&format!("1 .M {HEX} a.rs;"));
        let collapsed: HashSet<Group> = [Group::Changes].into_iter().collect();
        assert_eq!(items(&status, &collapsed), [Item::Header { group: Group::Changes, n_rows: 1 }]);
        assert!(items(&Status::default(), &HashSet::new()).is_empty());
    }

    #[test]
    fn test_c_review_3_the_merge_header_plus_stages_only_the_conflict_paths() {
        let with_conflict =
            status(&format!("u UU N... 100644 100644 100644 100644 1 2 3 c.rs;1 .M {HEX} a.rs;? n.txt;"));
        assert_eq!(paths_to_stage(&with_conflict, Group::Merge), Some(vec!["c.rs".to_string()]));
        assert_eq!(
            paths_to_stage(&with_conflict, Group::Changes),
            Some(vec!["a.rs".to_string(), "n.txt".to_string()]),
            "-A would mark the conflict as resolved"
        );
        let without = status(&format!("1 .M {HEX} a.rs;"));
        assert_eq!(paths_to_stage(&without, Group::Changes), None);
    }

    #[test]
    fn test_c_d53_paths_split_into_a_name_and_a_dimmed_folder() {
        assert_eq!(split_path("src/a/b.rs"), ("b.rs".into(), "src/a".into()));
        assert_eq!(split_path("README.md"), ("README.md".into(), String::new()));
        assert_eq!(split_path("한글 폴더/새 파일.txt"), ("새 파일.txt".into(), "한글 폴더".into()));
    }

    #[test]
    fn test_c_d53_the_badge_hides_at_zero_and_caps_at_ninety_nine() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(18).as_deref(), Some("18"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
    }

    #[test]
    fn test_c_d53_the_commit_button_needs_staged_changes_and_a_message() {
        assert_eq!(commit_state(0, "msg"), CommitState::NothingStaged);
        assert_eq!(commit_state(2, "  \n"), CommitState::NeedsMessage);
        assert_eq!(commit_state(2, "msg"), CommitState::Ready);
    }

    #[test]
    fn test_c_d53_the_discard_prompt_tells_a_restore_from_a_delete() {
        let tracked =
            Change { path: "src/a.rs".into(), orig_path: None, index: '.', worktree: 'M', section: Section::Changes };
        let prompt = discard_prompt(&tracked);
        assert_eq!((prompt.headline.as_str(), prompt.confirm), ("Discard changes in a.rs?", "Discard"));
        assert!(prompt.detail.contains("can't be undone"));
        let untracked = Change { path: "new.txt".into(), section: Section::Untracked, ..tracked };
        let prompt = discard_prompt(&untracked);
        assert_eq!((prompt.headline.as_str(), prompt.confirm), ("Delete untracked file new.txt?", "Delete"));
        assert!(prompt.detail.contains("can't be undone"));
    }
}
