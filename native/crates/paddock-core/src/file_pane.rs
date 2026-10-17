//! 파일 칸의 모델: 묶음(작업 폴더 또는 Unassigned 묶음)마다 터미널 배치 오른쪽에 두는 파일 탭 한 줄과 그 칸의 크기 비율.
//!
//! 터미널 배치(`layout_model`)는 터미널 번호만 쥐므로 파일 탭은 그 배치를 건드리지 않고 이 모델이 따로 쥔다.
//! 파일 칸은 묶음이 이름표(`GroupKey`)로 가리키고, 묶음이 폴더로 올라가거나 Unassigned 묶음의 시작 터미널이 닫혀
//! 이름표가 바뀌면 `follow_groups`가 칸을 새 이름표로 옮긴다.
use crate::file_kind::FileKind;
use crate::work_model::{GroupKey, WorkState};

/// 파일 칸이 처음 차지하는 본문 폭 비율.
pub const DEFAULT_RATIO: f32 = 0.5;

/// 파일 칸 비율의 하한. 상한은 `1 - MIN_RATIO`다. 터미널 쪽 칸이 같은 만큼 남는다.
pub const MIN_RATIO: f32 = 0.1;

/// 파일 탭 하나. `kind`는 탭이 보여 주는 방식이다(사용자가 이진 안내 탭을 텍스트로 열면 `Text`로 바뀐다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTab {
    pub path: String,
    pub kind: FileKind,
}

impl FileTab {
    /// 탭을 구별하는 값. 같은 파일의 본문 탭·비교 탭·스테이징된 비교 탭은 서로 다른 탭이다. 본문 탭의 열쇠는 경로 그대로다.
    pub fn key(&self) -> String {
        tab_key(&self.path, self.kind)
    }
}

/// `path`를 `kind`로 보여 주는 탭의 열쇠.
pub fn tab_key(
    path: &str,
    kind: FileKind,
) -> String {
    match kind {
        FileKind::Diff { staged } => format!("{}\u{1f}{path}", if staged { "diff-staged" } else { "diff" }),
        _ => path.to_string(),
    }
}

/// 한 묶음의 파일 칸.
#[derive(Clone, Debug, PartialEq)]
pub struct FilePane {
    /// 탭 순서. 비어 있는 칸은 두지 않는다.
    pub tabs: Vec<FileTab>,
    /// 선택한 탭의 번호.
    pub selected: usize,
    /// 칸이 차지하는 본문 폭 비율(`MIN_RATIO`..`1 - MIN_RATIO`).
    pub ratio: f32,
}

/// 묶음마다의 파일 칸 모음.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilePanes {
    panes: Vec<(GroupKey, FilePane)>,
}

/// 비율을 허용 범위로 맞춘다. 숫자가 아니면 기본 비율이다.
fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_nan() { DEFAULT_RATIO } else { ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO) }
}

impl FilePanes {
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// 파일 칸이 있는 묶음 수.
    pub fn len(&self) -> usize {
        self.panes.len()
    }

    /// 칸이 있는 묶음과 그 칸(저장 순서).
    pub fn iter(&self) -> impl Iterator<Item = &(GroupKey, FilePane)> {
        self.panes.iter()
    }

    pub fn get(
        &self,
        key: &GroupKey,
    ) -> Option<&FilePane> {
        self.panes.iter().find(|(owner, _)| owner == key).map(|(_, pane)| pane)
    }

    fn get_mut(
        &mut self,
        key: &GroupKey,
    ) -> Option<&mut FilePane> {
        self.panes.iter_mut().find(|(owner, _)| owner == key).map(|(_, pane)| pane)
    }

    /// 묶음에 열린 파일 탭 수.
    pub fn n_open_in(
        &self,
        key: &GroupKey,
    ) -> usize {
        self.get(key).map_or(0, |pane| pane.tabs.len())
    }

    /// 어느 묶음에서든 열려 있는 탭의 열쇠(같은 탭이 두 묶음에 열려 있으면 두 번 나온다).
    pub fn open_keys(&self) -> impl Iterator<Item = String> {
        self.panes.iter().flat_map(|(_, pane)| pane.tabs.iter().map(FileTab::key))
    }

    /// 저장된 칸을 되살릴 때 쓴다. 같은 묶음의 칸이 이미 있으면 바꾸고, 탭이 없는 칸은 두지 않는다.
    pub fn insert(
        &mut self,
        key: &GroupKey,
        mut pane: FilePane,
    ) {
        self.panes.retain(|(owner, _)| owner != key);
        if !pane.tabs.is_empty() {
            pane.selected = pane.selected.min(pane.tabs.len() - 1);
            pane.ratio = clamp_ratio(pane.ratio);
            self.panes.push((key.clone(), pane));
        }
    }

    /// 파일을 묶음의 칸에 연다. 이미 열려 있으면 그 탭을 선택만 한다. 선택한 탭 번호를 돌려준다.
    pub fn open(
        &mut self,
        key: &GroupKey,
        path: &str,
        kind: FileKind,
    ) -> usize {
        if self.get(key).is_none() {
            self.panes.push((key.clone(), FilePane { tabs: Vec::new(), selected: 0, ratio: DEFAULT_RATIO }));
        }
        let pane = self.get_mut(key).expect("pane was just ensured");
        let wanted = tab_key(path, kind);
        let index = match pane.tabs.iter().position(|tab| tab.key() == wanted) {
            Some(index) => index,
            None => {
                pane.tabs.push(FileTab { path: path.to_string(), kind });
                pane.tabs.len() - 1
            }
        };
        pane.selected = index;
        index
    }

    /// 탭을 선택한다. 없는 탭이면 false다.
    pub fn select(
        &mut self,
        key: &GroupKey,
        index: usize,
    ) -> bool {
        match self.get_mut(key) {
            Some(pane) if index < pane.tabs.len() => {
                pane.selected = index;
                true
            }
            _ => false,
        }
    }

    /// 탭을 닫는다. 선택한 탭을 닫았으면 그 자리에 들어온 다음 탭을, 없으면 이전 탭을 선택하고, 마지막 탭이면 칸을 없앤다.
    /// 닫을 탭이 없으면 false다.
    pub fn close(
        &mut self,
        key: &GroupKey,
        index: usize,
    ) -> bool {
        let mut closed = false;
        if let Some(pane) = self.get_mut(key)
            && index < pane.tabs.len()
        {
            pane.tabs.remove(index);
            if index < pane.selected || pane.selected >= pane.tabs.len() {
                pane.selected = pane.selected.saturating_sub(1);
            }
            closed = true;
        }
        self.panes.retain(|(_, pane)| !pane.tabs.is_empty());
        closed
    }

    /// 한 탭을 보여 주는 방식을 바꾼다(이진 안내 탭 → 텍스트).
    pub fn set_kind(
        &mut self,
        key: &GroupKey,
        index: usize,
        kind: FileKind,
    ) {
        if let Some(tab) = self.get_mut(key).and_then(|pane| pane.tabs.get_mut(index)) {
            tab.kind = kind;
        }
    }

    /// 칸의 폭 비율을 바꾼다. 허용 범위 밖의 값은 가장 가까운 한도로 맞춘다.
    pub fn set_ratio(
        &mut self,
        key: &GroupKey,
        ratio: f32,
    ) {
        if let Some(pane) = self.get_mut(key) {
            pane.ratio = clamp_ratio(ratio);
        }
    }

    /// 묶음 이름표가 바뀌었을 때 칸을 `to`로 옮긴다. `to`에 이미 칸이 있으면 `from`의 탭 중 없는 것만 뒤에 붙이고
    /// `to`의 선택·비율을 지킨다.
    pub fn rekey(
        &mut self,
        from: &GroupKey,
        to: &GroupKey,
    ) {
        if from != to
            && let Some(at) = self.panes.iter().position(|(owner, _)| owner == from)
        {
            let (_, moving) = self.panes.remove(at);
            match self.get_mut(to) {
                Some(existing) => {
                    for tab in moving.tabs {
                        if !existing.tabs.iter().any(|other| other.path == tab.path) {
                            existing.tabs.push(tab);
                        }
                    }
                }
                None => self.panes.push((to.clone(), moving)),
            }
        }
    }

    /// 묶음의 칸을 없애고 닫힌 탭 수를 돌려준다.
    pub fn close_group(
        &mut self,
        key: &GroupKey,
    ) -> usize {
        let n_closed = self.n_open_in(key);
        self.panes.retain(|(owner, _)| owner != key);
        n_closed
    }

    /// 작업 목록이 `before`에서 `after`로 바뀐 뒤 칸을 바뀐 이름표로 옮긴다.
    ///
    /// Unassigned 묶음(`Group`)의 칸은 이전 묶음의 터미널이 지금 속한 묶음으로 따라가므로, 묶음이 폴더로 올라가거나 시작 터미널이
    /// 닫혀 이름표가 바뀌어도 이어지고, 묶음이 사라졌으면 칸도 사라진다. 폴더 칸은 폴더가 목록에 있는 동안 남는다.
    pub fn follow_groups(
        &mut self,
        before: &WorkState,
        after: &WorkState,
    ) {
        let moves: Vec<(GroupKey, Option<GroupKey>)> = self
            .panes
            .iter()
            .map(|(key, _)| {
                let next = match key {
                    GroupKey::Folder(folder) => {
                        after.folders.iter().any(|candidate| candidate.key == *folder).then(|| key.clone())
                    }
                    GroupKey::Group(root) => {
                        before.group_members(*root).into_iter().find_map(|member| after.group_key_of(member))
                    }
                };
                (key.clone(), next)
            })
            .collect();
        for (key, next) in moves {
            match next {
                Some(next) => self.rekey(&key, &next),
                None => {
                    self.close_group(&key);
                }
            }
        }
    }
}

/// 새로 여는 파일이 속할 묶음. 작업 폴더가 파일을 품으면 가장 가까운 그 폴더이고, 아니면 지금 보는 묶음, 그것도 없으면
/// 맨 위에 보이는 터미널의 묶음이다. 경로가 비었거나 묶음으로 삼을 곳이 없으면 None이다.
///
/// Examples
/// --------
/// | 작업 폴더           | 경로                  | 지금 묶음 | 위 터미널 묶음 | 결과                  |
/// |---------------------|-----------------------|-----------|----------------|-----------------------|
/// | `~/repo`            | `/tmp/x.ts`           | 묶음 1    | 묶음 2         | 묶음 1                |
/// | `~/repo`            | `/tmp/x.ts`           | (없음)    | 묶음 2         | 묶음 2                |
/// | `~`, `~/repo`       | `~/repo/a.ts`         | 묶음 1    | (없음)         | 폴더 `~/repo`         |
/// | `~/repo`            | `/tmp/x.ts`           | (없음)    | (없음)         | None                  |
pub fn file_root_for(
    work: &WorkState,
    path: &str,
    current_group: Option<&GroupKey>,
    top_terminal_group: Option<&GroupKey>,
) -> Option<GroupKey> {
    if path.is_empty() {
        None
    } else {
        work.folder_containing(path)
            .map(|folder| GroupKey::Folder(folder.to_string()))
            .or_else(|| current_group.or(top_terminal_group).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_model::WorkState;

    fn folder(path: &str) -> GroupKey {
        GroupKey::Folder(path.to_string())
    }

    fn paths(pane: &FilePane) -> Vec<&str> {
        pane.tabs.iter().map(|tab| tab.path.as_str()).collect()
    }

    fn work_with_repo() -> WorkState {
        let mut work = WorkState::default();
        work.ensure_folder("/home/me/repo");
        work
    }

    #[test]
    fn test_fo01_a_file_outside_every_work_folder_joins_the_current_group_first() {
        let work = work_with_repo();
        let owner = file_root_for(&work, "/tmp/x.ts", Some(&GroupKey::Group(1)), Some(&GroupKey::Group(2)));
        assert_eq!(owner, Some(GroupKey::Group(1)));
    }

    #[test]
    fn test_fo02_without_a_current_group_the_top_terminal_group_owns_the_file() {
        let work = work_with_repo();
        assert_eq!(file_root_for(&work, "/tmp/x.ts", None, Some(&GroupKey::Group(2))), Some(GroupKey::Group(2)));
    }

    #[test]
    fn test_fo03_a_file_inside_a_work_folder_belongs_to_that_folder_even_when_a_group_is_shown() {
        let mut work = work_with_repo();
        work.ensure_folder("/home/me");
        let owner = file_root_for(&work, "/home/me/repo/a.ts", Some(&GroupKey::Group(1)), None);
        assert_eq!(owner, Some(folder("/home/me/repo")), "the nearest folder wins");
    }

    #[test]
    fn test_fo04_no_owner_when_no_group_exists_or_the_path_is_empty() {
        let work = work_with_repo();
        assert_eq!(file_root_for(&work, "/tmp/x.ts", None, None), None);
        assert_eq!(file_root_for(&work, "", Some(&GroupKey::Group(1)), None), None);
    }

    #[test]
    fn test_c_pane_1_open_adds_a_selected_tab_to_a_new_pane_with_the_default_ratio() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Text), 0);
        assert_eq!(panes.open(&key, "/a/y.rs", FileKind::Text), 1);
        let pane = panes.get(&key).unwrap();
        assert_eq!(paths(pane), vec!["/a/x.rs", "/a/y.rs"]);
        assert_eq!((pane.selected, pane.ratio), (1, DEFAULT_RATIO));
    }

    #[test]
    fn test_c_pane_2_opening_the_same_file_again_selects_the_existing_tab() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        panes.open(&key, "/a/x.rs", FileKind::Text);
        panes.open(&key, "/a/y.rs", FileKind::Text);
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Text), 0);
        let pane = panes.get(&key).unwrap();
        assert_eq!((pane.tabs.len(), pane.selected), (2, 0));
    }

    #[test]
    fn test_c_pane_3_closing_the_selected_tab_selects_the_one_that_took_its_place_then_the_previous() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        for name in ["1", "2", "3", "4"] {
            panes.open(&key, &format!("/a/{name}"), FileKind::Text);
        }
        panes.select(&key, 2);
        panes.close(&key, 2);
        assert_eq!(panes.get(&key).unwrap().selected, 2, "tab 4 took the place of tab 3");
        panes.close(&key, 2);
        assert_eq!(panes.get(&key).unwrap().selected, 1, "no later tab, so the previous one");
        panes.select(&key, 0);
        panes.close(&key, 1);
        assert_eq!(panes.get(&key).unwrap().selected, 0, "closing another tab keeps the selection");
    }

    #[test]
    fn test_c_pane_4_closing_the_last_tab_removes_the_pane() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        panes.open(&key, "/a/x.rs", FileKind::Text);
        assert!(panes.close(&key, 0));
        assert!(panes.get(&key).is_none());
        assert!(panes.is_empty());
        assert!(!panes.close(&key, 0), "nothing left to close");
    }

    #[test]
    fn test_c_pane_5_rekey_moves_the_pane_with_its_ratio_and_merges_into_an_existing_one() {
        let mut panes = FilePanes::default();
        let (group, target) = (GroupKey::Group(3), folder("/a"));
        panes.open(&group, "/a/x.rs", FileKind::Text);
        panes.open(&group, "/a/y.rs", FileKind::Text);
        panes.set_ratio(&group, 0.3);
        panes.rekey(&group, &target);
        assert!(panes.get(&group).is_none());
        let moved = panes.get(&target).unwrap();
        assert_eq!((paths(moved), moved.selected, moved.ratio), (vec!["/a/x.rs", "/a/y.rs"], 1, 0.3));
        // 이미 파일 칸이 있는 폴더로 옮기면 없는 탭만 뒤에 붙고, 그 폴더의 비율·선택이 이긴다.
        panes.open(&GroupKey::Group(9), "/a/y.rs", FileKind::Text);
        panes.open(&GroupKey::Group(9), "/a/z.rs", FileKind::Text);
        panes.rekey(&GroupKey::Group(9), &target);
        let merged = panes.get(&target).unwrap();
        assert_eq!((paths(merged), merged.selected, merged.ratio), (vec!["/a/x.rs", "/a/y.rs", "/a/z.rs"], 1, 0.3));
        assert_eq!(panes.len(), 1);
    }

    #[test]
    fn test_c_pane_6_ratio_stays_between_a_tenth_and_nine_tenths() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        panes.open(&key, "/a/x.rs", FileKind::Text);
        panes.set_ratio(&key, 0.0);
        assert_eq!(panes.get(&key).unwrap().ratio, MIN_RATIO);
        panes.set_ratio(&key, 2.0);
        assert_eq!(panes.get(&key).unwrap().ratio, 1.0 - MIN_RATIO);
        panes.set_ratio(&key, f32::NAN);
        assert_eq!(panes.get(&key).unwrap().ratio, DEFAULT_RATIO);
    }

    #[test]
    fn test_set_kind_changes_how_one_tab_is_shown_without_moving_it() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        panes.open(&key, "/a/x.zip", FileKind::Binary);
        panes.set_kind(&key, 0, FileKind::Text);
        assert_eq!(panes.get(&key).unwrap().tabs[0].kind, FileKind::Text);
    }

    #[test]
    fn test_follow_groups_moves_a_group_pane_to_the_folder_its_terminals_were_promoted_to() {
        let mut before = WorkState::default();
        before.add_group(2, "terminal 1");
        before.join_group(2, 5, "terminal 2");
        let mut after = before.clone();
        after.assign_terminal(2, "/home/me/app", "claude");
        after.assign_terminal(5, "/home/me/app", "terminal 2");
        let mut panes = FilePanes::default();
        panes.open(&GroupKey::Group(2), "/tmp/x.ts", FileKind::Text);
        panes.follow_groups(&before, &after);
        assert!(panes.get(&GroupKey::Group(2)).is_none());
        assert_eq!(paths(panes.get(&folder("/home/me/app")).unwrap()), vec!["/tmp/x.ts"]);
    }

    #[test]
    fn test_follow_groups_keeps_the_pane_when_the_start_terminal_closes_and_drops_it_with_the_last_one() {
        let mut before = WorkState::default();
        before.add_group(2, "terminal 1");
        before.join_group(2, 5, "terminal 2");
        let mut panes = FilePanes::default();
        panes.open(&GroupKey::Group(2), "/tmp/x.ts", FileKind::Text);
        let mut after = before.clone();
        after.forget_terminal(2);
        panes.follow_groups(&before, &after);
        assert!(panes.get(&GroupKey::Group(2)).is_none());
        assert_eq!(paths(panes.get(&GroupKey::Group(5)).unwrap()), vec!["/tmp/x.ts"]);
        let mut last = after.clone();
        last.forget_terminal(5);
        panes.follow_groups(&after, &last);
        assert!(panes.is_empty(), "the group is gone, so are its files");
    }

    #[test]
    fn test_follow_groups_keeps_folder_panes_until_the_folder_leaves_the_list() {
        let mut before = work_with_repo();
        before.assign_terminal(1, "/home/me/repo", "claude");
        let mut panes = FilePanes::default();
        panes.open(&folder("/home/me/repo"), "/home/me/repo/a.ts", FileKind::Text);
        let mut after = before.clone();
        after.forget_terminal(1);
        panes.follow_groups(&before, &after);
        assert!(panes.get(&folder("/home/me/repo")).is_some(), "a folder keeps its files without terminals");
        let mut removed = after.clone();
        removed.remove_folder("/home/me/repo", None);
        panes.follow_groups(&after, &removed);
        assert!(panes.is_empty());
    }

    #[test]
    fn test_c_d53_a_diff_tab_and_a_text_tab_of_the_same_file_are_different_tabs_and_staged_differs_from_unstaged() {
        let mut panes = FilePanes::default();
        let key = folder("/a");
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Text), 0);
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Diff { staged: false }), 1);
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Diff { staged: true }), 2);
        assert_eq!(panes.open(&key, "/a/x.rs", FileKind::Diff { staged: false }), 1, "the same diff selects its tab");
        let keys: std::collections::HashSet<String> = panes.open_keys().collect();
        assert_eq!(keys.len(), 3);
        assert_eq!(panes.get(&key).unwrap().tabs[1].path, "/a/x.rs", "a diff tab keeps the real path for ownership");
    }

    #[test]
    fn test_n_open_in_counts_tabs_of_one_group_and_open_paths_lists_every_group() {
        let mut panes = FilePanes::default();
        panes.open(&folder("/a"), "/a/x", FileKind::Text);
        panes.open(&GroupKey::Group(1), "/a/x", FileKind::Text);
        panes.open(&GroupKey::Group(1), "/b/y", FileKind::Text);
        assert_eq!(panes.n_open_in(&GroupKey::Group(1)), 2);
        assert_eq!(panes.n_open_in(&GroupKey::Group(7)), 0);
        let mut open: Vec<String> = panes.open_keys().collect();
        open.sort_unstable();
        assert_eq!(open, vec!["/a/x", "/a/x", "/b/y"]);
    }
}
