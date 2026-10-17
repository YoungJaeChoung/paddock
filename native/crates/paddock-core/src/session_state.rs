//! 앱이 종료 때 남기는 state.json 모델: 터미널을 쥔 데몬의 식별 글자, 작업 폴더·폴더 안 터미널·Unassigned 묶음, 선택한 터미널.
//!
//! 재실행 때 같은 데몬에 아직 살아 있는 터미널만 되살린다. 터미널 id는 데몬마다 1부터 매겨져 다른 데몬의 id와 겹치므로,
//! 저장한 데몬과 지금 데몬이 다르면 저장 내용을 잇지 않는다. 파일이 깨져도 앱이 시작되도록 읽기는 실패하지 않으며,
//! 모양이 틀린 항목은 그 항목만 버린다.
//!
//! 형식(버전 3): `{version, daemon, work: {folders, terminals}, groups, selected_terminal, layouts, sidebar_collapsed, files_collapsed}`.
//! `layouts`는 칸 배치(분할) 목록이다. 배치는 묶음 이름이 아니라 든 터미널 번호로 묶음을 찾으므로, 묶음의 시작 터미널이
//! 바뀌어도 배치가 이어진다. 버전 1(평평한 `terminals` 목록)은 폴더·묶음이 없는 파일이라 빈 목록으로 읽는다.
//!
//! 파일 칸(`files`: `{tabs: [{path}], selected, ratio}`)은 최상위 목록이 아니라 그 칸이 속한 폴더 항목과 묶음 항목 안에 둔다.
//! 묶음의 시작 터미널이 되살아나지 못해 시작 터미널이 바뀌어도 칸이 묶음 항목과 함께 이어지고, 묶음이 사라지면 칸도 함께 사라진다.
//! 버전 2 파일은 파일 칸 없이 읽고, 다시 저장하면 버전 3이 된다.
use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};

use crate::file_kind::{FileKind, kind_of};
use crate::file_pane::{DEFAULT_RATIO, FilePane, FilePanes, FileTab};
use crate::layout_model::Layout;
use crate::work_model::{Group, GroupKey, GroupMember, Member, WorkState, same_folder};

/// state.json의 현재 형식 번호.
const VERSION: u64 = 3;

/// 읽을 수 있는 가장 오래된 형식 번호. 이 번호부터 현재 번호까지는 모두 읽는다.
const OLDEST_READABLE_VERSION: u64 = 2;

/// 저장하는 상태.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionState {
    /// 이 터미널들을 쥔 데몬의 식별 글자. 터미널이 하나도 없던 상태는 빈 글자다.
    pub daemon: String,
    pub work: WorkState,
    pub selected: Option<u32>,
    /// 칸 배치(분할) 목록. 한 터미널은 한 배치에만 든다. 배치가 없는 묶음은 터미널마다 탭인 한 칸으로 본다.
    pub layouts: Vec<Layout>,
    /// 왼쪽 사이드바가 접혀 있다.
    pub sidebar_collapsed: bool,
    /// 묶음마다의 파일 칸(열린 파일 탭·선택·폭 비율).
    pub files: FilePanes,
    /// 사이드바의 파일 목록 구획이 접혀 있다.
    pub files_collapsed: bool,
}

fn as_id(value: Option<&Value>) -> Option<u32> {
    value.and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok())
}

impl SessionState {
    /// 상태에 든 모든 터미널 id. 작업 폴더의 터미널이 먼저이고 Unassigned 묶음이 뒤따른다.
    pub fn terminal_ids(&self) -> Vec<u32> {
        let in_folders = self.work.terminals.iter().map(|member| member.id);
        let in_groups = self.work.groups.iter().flat_map(|group| group.members.iter()).map(|member| member.id);
        in_folders.chain(in_groups).collect()
    }

    /// state.json에 쓸 문자열.
    pub fn serialize(&self) -> String {
        let folders: Vec<Value> = self
            .work
            .folders
            .iter()
            .map(|folder| {
                let mut entry = json!({"key": folder.key, "expanded": folder.expanded, "label": folder.label});
                if let Some(pane) = self.files.get(&GroupKey::Folder(folder.key.clone())) {
                    entry["files"] = serialize_pane(pane);
                }
                entry
            })
            .collect();
        let terminals: Vec<Value> = self
            .work
            .terminals
            .iter()
            .map(|member| json!({"id": member.id, "folder": member.folder, "name": member.name}))
            .collect();
        let groups: Vec<Value> = self
            .work
            .groups
            .iter()
            .map(|group| {
                let members: Vec<Value> =
                    group.members.iter().map(|member| json!({"id": member.id, "name": member.name})).collect();
                let mut entry = json!({"root": group.root, "members": members, "last_used": group.last_used});
                if let Some(pane) = self.files.get(&GroupKey::Group(group.root)) {
                    entry["files"] = serialize_pane(pane);
                }
                entry
            })
            .collect();
        let document = json!({
            "version": VERSION,
            "daemon": self.daemon,
            "work": {"folders": folders, "terminals": terminals},
            "groups": groups,
            "selected_terminal": self.selected,
            "layouts": self.layouts.iter().map(|layout| Layout::serialize(Some(layout))).collect::<Vec<_>>(),
            "sidebar_collapsed": self.sidebar_collapsed,
            "files_collapsed": self.files_collapsed,
        });
        serde_json::to_string_pretty(&document).expect("session state is plain data")
    }

    /// 저장 문자열에서 상태를 되살린다. `live_ids`(지금 데몬에 있는 터미널)에 있는 터미널만 남기고 저장 순서를 유지한다.
    ///
    /// 저장한 데몬이 `daemon`(지금 데몬)과 다르면 터미널 소속·선택·배치는 없고 작업 폴더 목록과 사이드바 접힘만 남는다.
    /// 형식이 깨진 파일은 빈 상태이고, 모양이 틀린 항목·중복 id·없는 터미널은
    /// 그 항목만 버린다. 터미널이 모두 없어진 작업 폴더는 남고, 터미널이 모두 없어진 묶음은 사라진다.
    /// 선택한 터미널이 버려졌으면 선택은 None이다. 배치는 살아 있는 터미널만 남기고, 한 터미널이 여러 배치에 있으면 앞 배치가 쥔다.
    /// 파일 칸은 폴더 항목의 것은 데몬과 무관하게 남기고, 묶음 항목의 것은 그 묶음이 되살아날 때만 남긴다. 모양이 틀린 파일 탭은 그 탭만 버린다.
    ///
    /// Examples
    /// --------
    /// | 저장 내용                               | live_ids | 결과                              |
    /// |-----------------------------------------|----------|-----------------------------------|
    /// | `{not json`                             | {1}      | 빈 상태                           |
    /// | 다른 데몬이 저장한 터미널 1, 폴더 2개   | {1}      | 폴더 2개만 (터미널·선택 없음)     |
    /// | 폴더 터미널 1·2, selected 1             | {2}      | 폴더 터미널 2, selected None      |
    /// | 버전 1 파일(평평한 터미널 목록)         | {1, 2}   | 폴더·묶음 없음                    |
    /// | 묶음 하나가 모양이 틀림                 | {1, 2}   | 그 묶음만 버리고 나머지 유지      |
    pub fn restore(
        text: &str,
        live_ids: &HashSet<u32>,
        daemon: &str,
    ) -> Self {
        let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let mut state = Self::default();
        let same_daemon = parsed.get("daemon").and_then(Value::as_str) == Some(daemon);
        if same_daemon {
            state.daemon = daemon.to_string();
        }
        let version = parsed.get("version").and_then(Value::as_u64);
        if version.is_some_and(|version| (OLDEST_READABLE_VERSION..=VERSION).contains(&version)) {
            // 터미널 번호는 데몬마다 새로 매겨지므로 터미널 소속·선택·배치는 같은 데몬이 저장한 것일 때만 잇는다.
            // 작업 폴더 목록과 사이드바 접힘은 번호와 무관해 데몬이 바뀌거나 탭이 하나도 없어도 남긴다.
            let no_terminals = HashSet::new();
            let live = if same_daemon { live_ids } else { &no_terminals };
            let mut seen = HashSet::new();
            state.work = restore_work(&parsed, live, &mut seen, &mut state.files);
            state.selected = as_id(parsed.get("selected_terminal")).filter(|id| seen.contains(id));
            state.layouts = restore_layouts(&parsed, &seen);
            state.sidebar_collapsed = parsed.get("sidebar_collapsed").and_then(Value::as_bool).unwrap_or(false);
            state.files_collapsed = parsed.get("files_collapsed").and_then(Value::as_bool).unwrap_or(false);
        }
        state
    }
}

impl SessionState {
    /// 저장 문자열을 터미널이 지금 있는지 가리지 않고 저장된 그대로 읽는다. 데몬이 바뀐 뒤 옛 데몬 시절의 터미널 자리를 알아내는 데 쓴다.
    ///
    /// `daemon`은 저장한 데몬의 식별 글자 그대로다. 형식이 깨진 파일은 빈 상태다.
    pub fn restore_saved(text: &str) -> Self {
        let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let daemon = parsed.get("daemon").and_then(Value::as_str).unwrap_or("").to_string();
        let mut every_id = HashSet::new();
        let saved_work = parsed.get("work");
        let in_folders = saved_work.and_then(|work| work.get("terminals")).and_then(Value::as_array);
        let in_groups = parsed.get("groups").and_then(Value::as_array);
        every_id.extend(in_folders.into_iter().flatten().filter_map(|entry| as_id(entry.get("id"))));
        every_id.extend(
            in_groups
                .into_iter()
                .flatten()
                .flat_map(|group| group.get("members").and_then(Value::as_array).into_iter().flatten())
                .filter_map(|member| as_id(member.get("id"))),
        );
        Self::restore(text, &every_id, &daemon)
    }

    /// 터미널 번호를 `new_ids`(옛 번호 → 새 번호)로 바꾼 상태. 새 번호가 없는 터미널은 소속·선택·배치에서 뺀다.
    ///
    /// 터미널을 되살리면 새 데몬이 번호를 새로 매기므로, 저장된 폴더·묶음·이름·배치를 새 번호로 옮겨 같은 자리에 놓는 데 쓴다.
    /// 터미널이 모두 빠진 작업 폴더는 남고, 터미널이 모두 빠진 묶음은 사라진다. 묶음의 시작 터미널이 빠졌으면 남은 첫 터미널이 시작 터미널이다.
    ///
    /// Examples
    /// --------
    /// | 상태                                    | `new_ids`   | 결과                                   |
    /// |-----------------------------------------|-------------|----------------------------------------|
    /// | 폴더 터미널 3, 묶음 [2, 5], 선택 3      | 3→30, 5→50  | 폴더 터미널 30, 묶음 [50], 선택 30     |
    /// | 폴더 터미널 3                           | (없음)      | 폴더만 남고 터미널·선택·배치는 없음    |
    pub fn renumbered(
        &self,
        new_ids: &BTreeMap<u32, u32>,
        daemon: &str,
    ) -> Self {
        let new_id = |id: u32| new_ids.get(&id).copied();
        let mut work = WorkState { folders: self.work.folders.clone(), ..WorkState::default() };
        work.terminals = self
            .work
            .terminals
            .iter()
            .filter_map(|member| new_id(member.id).map(|id| Member { id, ..member.clone() }))
            .collect();
        let mut files = FilePanes::default();
        for (key, pane) in self.files.iter() {
            if matches!(key, GroupKey::Folder(_)) {
                files.insert(key, pane.clone());
            }
        }
        for group in &self.work.groups {
            let members: Vec<GroupMember> = group
                .members
                .iter()
                .filter_map(|member| new_id(member.id).map(|id| GroupMember { id, name: member.name.clone() }))
                .collect();
            if let Some(first) = members.first() {
                let root = first.id;
                let last_used = new_id(group.last_used).unwrap_or(root);
                work.groups.push(Group { root, members, last_used });
                if let Some(pane) = self.files.get(&GroupKey::Group(group.root)) {
                    files.insert(&GroupKey::Group(root), pane.clone());
                }
            }
        }
        Self {
            daemon: daemon.to_string(),
            work,
            selected: self.selected.and_then(new_id),
            layouts: self.layouts.iter().filter_map(|layout| layout.renumbered(new_id)).collect(),
            sidebar_collapsed: self.sidebar_collapsed,
            files,
            files_collapsed: self.files_collapsed,
        }
    }
}

/// 파일 칸을 저장 값으로 만든다.
///
/// 변경 비교 탭은 저장하지 않는다 — 다시 켰을 때의 변경은 그때의 git 상태가 정하므로 낡은 비교를 되살리지 않는다.
fn serialize_pane(pane: &FilePane) -> Value {
    let kept: Vec<(usize, &FileTab)> =
        pane.tabs.iter().enumerate().filter(|(_, tab)| !matches!(tab.kind, FileKind::Diff { .. })).collect();
    let tabs: Vec<Value> = kept.iter().map(|(_, tab)| json!({"path": tab.path})).collect();
    let selected = kept.iter().position(|(index, _)| *index >= pane.selected).unwrap_or(kept.len().saturating_sub(1));
    json!({"tabs": tabs, "selected": selected, "ratio": pane.ratio})
}

/// 저장 값에서 파일 칸을 읽는다. 경로가 문자열이 아니거나 빈 탭은 그 탭만 버리고, 탭이 하나도 없으면 칸이 없다.
/// 탭이 보여 주는 방식은 저장하지 않고 경로의 확장자로 다시 정한다.
fn restore_pane(saved: Option<&Value>) -> Option<FilePane> {
    saved.and_then(|saved| {
        let tabs: Vec<FileTab> = saved
            .get("tabs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tab| tab.get("path").and_then(Value::as_str))
            .filter(|path| !path.is_empty())
            .map(|path| FileTab { path: path.to_string(), kind: kind_of(path) })
            .collect();
        let selected = saved.get("selected").and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok());
        let ratio = saved.get("ratio").and_then(Value::as_f64).map_or(DEFAULT_RATIO, |ratio| ratio as f32);
        (!tabs.is_empty()).then_some(FilePane { tabs, selected: selected.unwrap_or(0), ratio })
    })
}

/// 저장 값에서 칸 배치 목록을 읽는다. `restored_ids`(되살린 터미널)만 남기고, 먼저 읽은 배치가 쥔 터미널은 뒤 배치에서 뺀다.
fn restore_layouts(
    parsed: &Value,
    restored_ids: &HashSet<u32>,
) -> Vec<Layout> {
    let mut claimed = HashSet::new();
    let mut layouts = Vec::new();
    for entry in parsed.get("layouts").and_then(Value::as_array).into_iter().flatten() {
        let layout = Layout::restore(entry, |id| restored_ids.contains(&id))
            .and_then(|layout| layout.prune(|id| !claimed.contains(&id)));
        if let Some(layout) = layout {
            claimed.extend(layout.terminal_ids());
            layouts.push(layout);
        }
    }
    layouts
}

/// 저장 값에서 작업 폴더와 묶음을 읽는다. 읽은 터미널 id를 `seen`에 모아 중복을 거르고, 되살린 폴더·묶음의 파일 칸을 `files`에 담는다.
fn restore_work(
    parsed: &Value,
    live_ids: &HashSet<u32>,
    seen: &mut HashSet<u32>,
    files: &mut FilePanes,
) -> WorkState {
    let mut work = WorkState::default();
    let saved_work = parsed.get("work");
    for entry in saved_work.and_then(|work| work.get("folders")).and_then(Value::as_array).into_iter().flatten() {
        if let Some(key) = entry.get("key").and_then(Value::as_str) {
            work.ensure_folder(key);
            work.set_expanded(key, entry.get("expanded").and_then(Value::as_bool).unwrap_or(true));
            if let Some(label) = entry.get("label").and_then(Value::as_str) {
                work.set_folder_label(key, label);
            }
            let owner =
                work.folders.iter().find(|folder| same_folder(&folder.key, key)).map(|folder| folder.key.clone());
            if let (Some(owner), Some(pane)) = (owner, restore_pane(entry.get("files"))) {
                files.insert(&GroupKey::Folder(owner), pane);
            }
        }
    }
    for entry in saved_work.and_then(|work| work.get("terminals")).and_then(Value::as_array).into_iter().flatten() {
        let id = as_id(entry.get("id"));
        let folder = entry.get("folder").and_then(Value::as_str);
        let name = entry.get("name").and_then(Value::as_str);
        if let (Some(id), Some(folder), Some(name)) = (id, folder, name)
            && live_ids.contains(&id)
            && seen.insert(id)
        {
            work.assign_terminal(id, folder, name);
        }
    }
    for entry in parsed.get("groups").and_then(Value::as_array).into_iter().flatten() {
        let mut members = Vec::new();
        for saved in entry.get("members").and_then(Value::as_array).into_iter().flatten() {
            let id = as_id(saved.get("id"));
            let name = saved.get("name").and_then(Value::as_str);
            if let (Some(id), Some(name)) = (id, name)
                && live_ids.contains(&id)
                && seen.insert(id)
            {
                members.push(GroupMember { id, name: name.to_string() });
            }
        }
        // 시작 터미널이 사라졌어도 남은 터미널로 묶음을 잇는다. 마지막으로 쓴 터미널이 사라졌으면 시작 터미널로 돌아간다.
        if as_id(entry.get("root")).is_some()
            && let Some(first) = members.first()
        {
            let root = first.id;
            let last_used = as_id(entry.get("last_used"))
                .filter(|id| members.iter().any(|member| member.id == *id))
                .unwrap_or(root);
            work.groups.push(Group { root, members, last_used });
            if let Some(pane) = restore_pane(entry.get("files")) {
                files.insert(&GroupKey::Group(root), pane);
            }
        }
    }
    work
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_kind::FileKind;
    use crate::file_pane::DEFAULT_RATIO;
    use crate::layout_model::Orientation;
    use crate::work_model::GroupKey;

    fn live(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    fn sample() -> SessionState {
        let mut work = WorkState::default();
        work.ensure_folder("/home/me/app");
        work.set_folder_label("/home/me/app", "앱");
        work.set_expanded("/home/me/app", false);
        work.ensure_folder("C:/work");
        work.assign_terminal(3, "/home/me/app", "claude");
        work.assign_terminal(1, "/home/me/app", "한글 이름");
        work.add_group(2, "terminal 2");
        work.join_group(2, 5, "terminal 3");
        work.touch(5);
        SessionState { daemon: "7@t0".into(), work, selected: Some(1), ..SessionState::default() }
    }

    fn pane(
        tabs: &[u32],
        selected: usize,
    ) -> Layout {
        Layout::pane(tabs.to_vec(), selected)
    }

    fn files_sample() -> SessionState {
        let mut state = sample();
        state.files.open(&GroupKey::Folder("/home/me/app".into()), "/home/me/app/src/a.ts", FileKind::Text);
        state.files.open(&GroupKey::Folder("/home/me/app".into()), "/home/me/app/b.zip", FileKind::Binary);
        state.files.set_ratio(&GroupKey::Folder("/home/me/app".into()), 0.35);
        state.files.open(&GroupKey::Group(2), "/tmp/notes.md", FileKind::Markdown);
        state.files_collapsed = true;
        state
    }

    #[test]
    fn test_c_d51_v3_files_roundtrip() {
        let state = files_sample();
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        assert_eq!(restored, state, "tabs, selection, ratio and the files section state come back");
        let pane = restored.files.get(&GroupKey::Folder("/home/me/app".into())).unwrap();
        assert_eq!((pane.selected, pane.ratio), (1, 0.35));
        assert!(restored.files_collapsed);
    }

    #[test]
    fn test_c_d53_diff_tabs_are_not_saved_and_the_selection_moves_to_a_kept_tab() {
        let mut state = sample();
        let key = GroupKey::Folder("/home/me/app".into());
        state.files.open(&key, "/home/me/app/a.ts", FileKind::Text);
        state.files.open(&key, "/home/me/app/a.ts", FileKind::Diff { staged: false });
        state.files.open(&key, "/home/me/app/b.ts", FileKind::Text);
        state.files.select(&key, 1);
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        let pane = restored.files.get(&key).unwrap();
        assert_eq!(
            pane.tabs.iter().map(|tab| tab.path.as_str()).collect::<Vec<_>>(),
            ["/home/me/app/a.ts", "/home/me/app/b.ts"]
        );
        assert!(pane.tabs.iter().all(|tab| !matches!(tab.kind, FileKind::Diff { .. })));
        assert_eq!(pane.selected, 1, "the selected diff tab is replaced by the next kept tab");
    }

    #[test]
    fn test_c_d51_v2_file_reads_without_files() {
        let v2 = r#"{"version":2,"daemon":"7@t0",
            "work":{"folders":[{"key":"/a","expanded":true}],"terminals":[{"id":1,"folder":"/a","name":"x"}]},
            "groups":[],"selected_terminal":1,"layouts":[],"sidebar_collapsed":true}"#;
        let restored = SessionState::restore(v2, &live(&[1]), "7@t0");
        assert_eq!(restored.work.terminals_of("/a"), vec![1]);
        assert_eq!(restored.selected, Some(1));
        assert!(restored.sidebar_collapsed);
        assert!(restored.files.is_empty() && !restored.files_collapsed);
        assert!(restored.serialize().contains("\"version\": 3"), "saving it writes the new format");
    }

    #[test]
    fn test_c_d51_a_v2_file_written_by_the_first_stage_restores_exactly_as_before() {
        // 1단계가 쓴 버전 2 파일(파일 칸·접힘 값이 없는 것)을 새 코드가 읽어도 폴더·묶음·배치·선택·사이드바가 그대로다.
        let mut state = sample();
        state.layouts = vec![pane(&[3, 1], 1), pane(&[2, 5], 0)];
        state.sidebar_collapsed = true;
        let mut document: Value = serde_json::from_str(&state.serialize()).unwrap();
        document["version"] = json!(2);
        document.as_object_mut().unwrap().remove("files_collapsed");
        let restored = SessionState::restore(&document.to_string(), &live(&[1, 2, 3, 5]), "7@t0");
        assert_eq!(restored, state);
        assert!(restored.files.is_empty());
    }

    #[test]
    fn test_c_d51_broken_file_entry_is_dropped_alone() {
        let text = r#"{"version":3,"daemon":"7@t0",
            "work":{"folders":[{"key":"/a","expanded":true,"files":{"tabs":[{"path":"/a/x.rs"},{"path":5},{"nope":1},"str",{"path":""},{"path":"/a/y.rs"}],"selected":"two","ratio":"wide"}},
                               {"key":"/b","expanded":true,"files":7}],"terminals":[]},
            "groups":[],"files_collapsed":"yes"}"#;
        let restored = SessionState::restore(text, &live(&[]), "7@t0");
        let pane = restored.files.get(&GroupKey::Folder("/a".into())).unwrap();
        assert_eq!(pane.tabs.iter().map(|tab| tab.path.as_str()).collect::<Vec<_>>(), vec!["/a/x.rs", "/a/y.rs"]);
        assert_eq!((pane.selected, pane.ratio), (0, DEFAULT_RATIO));
        assert!(restored.files.get(&GroupKey::Folder("/b".into())).is_none());
        assert!(!restored.files_collapsed, "a non-boolean flag is ignored");
    }

    #[test]
    fn test_c_d51_unassigned_files_are_dropped_when_the_daemon_changes() {
        let state = files_sample();
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "8@t1");
        assert!(restored.files.get(&GroupKey::Group(2)).is_none(), "the group is gone, so are its files");
        assert_eq!(restored.files.len(), 1);
    }

    #[test]
    fn test_c_d51_folder_files_survive_a_daemon_change() {
        let state = files_sample();
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "8@t1");
        assert_eq!(
            restored.files.get(&GroupKey::Folder("/home/me/app".into())),
            state.files.get(&GroupKey::Folder("/home/me/app".into()))
        );
        assert!(restored.files_collapsed);
    }

    #[test]
    fn test_c_d51_a_group_pane_follows_its_group_when_the_start_terminal_did_not_come_back() {
        // 묶음 [2, 5]의 파일 칸은 시작 터미널 2가 죽어 5가 시작 터미널이 되어도 묶음을 따라간다.
        let state = files_sample();
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 3, 5]), "7@t0");
        assert_eq!(restored.work.group_members(5), vec![5]);
        assert_eq!(restored.files.get(&GroupKey::Group(5)).map(|pane| pane.tabs.len()), Some(1));
        assert!(restored.files.get(&GroupKey::Group(2)).is_none());
    }

    #[test]
    fn test_c_d51_renumbering_moves_group_files_to_the_new_start_terminal() {
        let saved = SessionState::restore_saved(&files_sample().serialize());
        let map: BTreeMap<u32, u32> = [(3, 30), (2, 20), (5, 50)].into_iter().collect();
        let back = saved.renumbered(&map, "9@t1");
        assert_eq!(back.files.get(&GroupKey::Group(20)).map(|pane| pane.tabs.len()), Some(1));
        assert!(back.files.get(&GroupKey::Group(2)).is_none());
        assert_eq!(back.files.len(), 2);
        assert!(back.files_collapsed);
        // 묶음의 터미널이 하나도 되살아나지 못하면 그 묶음의 파일 칸도 없고 폴더 칸은 남는다.
        let gone = saved.renumbered(&[(3, 30)].into_iter().collect(), "9@t1");
        assert!(gone.files.get(&GroupKey::Group(20)).is_none());
        assert_eq!(gone.files.len(), 1);
    }

    #[test]
    fn test_c_d11b_state_roundtrip() {
        let state = sample();
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        assert_eq!(restored, state);
    }

    #[test]
    fn test_c_d40_a_saved_state_is_read_as_it_was_and_renumbered_for_the_terminals_that_came_back() {
        let mut state = sample();
        state.layouts = vec![pane(&[3, 1], 1), pane(&[2, 5], 0)];
        let saved = SessionState::restore_saved(&state.serialize());
        assert_eq!(saved, state, "every terminal of the old daemon is kept");
        // 3·2·5가 새 번호 30·20·50으로 되살아나고 1은 되살아나지 못했다.
        let map: BTreeMap<u32, u32> = [(3, 30), (2, 20), (5, 50)].into_iter().collect();
        let back = saved.renumbered(&map, "9@t1");
        assert_eq!(back.daemon, "9@t1");
        assert_eq!(back.work.terminals_of("/home/me/app"), vec![30]);
        assert_eq!(back.work.name_of(30), Some("claude"));
        assert_eq!(back.work.group_members(20), vec![20, 50]);
        assert_eq!(back.work.groups[0].last_used, 50, "the last used terminal follows its new number");
        assert_eq!(back.selected, None, "terminal 1 was selected but did not come back");
        assert_eq!(back.layouts, vec![pane(&[30], 0), pane(&[20, 50], 0)]);
        assert_eq!(back.work.folders.len(), 2, "folders do not depend on terminals");
        assert!(back.sidebar_collapsed == state.sidebar_collapsed);
        // 되살아난 터미널이 없으면 폴더 목록만 남는다.
        let none = saved.renumbered(&BTreeMap::new(), "9@t1");
        assert_eq!((none.work.terminals.len(), none.work.groups.len(), none.layouts.len()), (0, 0, 0));
        assert_eq!(none.work.folders.len(), 2);
        assert_eq!(SessionState::restore_saved("{oops"), SessionState::default());
    }

    #[test]
    fn test_c_d11b_v2_work_and_groups_roundtrip() {
        // 폴더(펼침·표시 이름·빈 폴더), 폴더 안 터미널 순서, Unassigned 묶음의 내부 순서와 마지막 쓴 터미널이 그대로 돌아온다.
        let restored = SessionState::restore(&sample().serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        assert_eq!(restored.work.folders.len(), 2);
        assert_eq!(restored.work.folders[0].label.as_deref(), Some("앱"));
        assert!(!restored.work.folders[0].expanded);
        assert_eq!(restored.work.terminals_of("/home/me/app"), vec![3, 1]);
        assert_eq!(restored.work.group_members(2), vec![2, 5]);
        assert_eq!(restored.work.groups[0].last_used, 5);
        assert_eq!(restored.work.name_of(5), Some("terminal 3"));
        assert_eq!(restored.selected, Some(1));
    }

    #[test]
    fn test_c_d38_layouts_and_sidebar_roundtrip_with_ratios_selection_and_collapse() {
        let mut state = sample();
        state.layouts = vec![
            Layout::Split {
                orientation: Orientation::Horizontal,
                children: vec![pane(&[3], 0), pane(&[1], 0)],
                sizes: vec![0.3, 0.7],
            },
            pane(&[2, 5], 1),
        ];
        state.sidebar_collapsed = true;
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        assert_eq!(restored, state);
        assert!(!SessionState::restore(&sample().serialize(), &live(&[1, 2, 3, 5]), "7@t0").sidebar_collapsed);
    }

    #[test]
    fn test_c_d38_layouts_keep_only_live_terminals_and_a_terminal_is_in_one_layout_only() {
        let mut state = sample();
        state.layouts = vec![
            Layout::Split {
                orientation: Orientation::Vertical,
                children: vec![pane(&[3], 0), pane(&[1, 99], 1)],
                sizes: vec![1.0, 1.0],
            },
            pane(&[1, 2], 0),
        ];
        let restored = SessionState::restore(&state.serialize(), &live(&[1, 2, 3, 5]), "7@t0");
        // 99는 죽었고, 1은 앞 배치가 먼저 쥐었으므로 뒤 배치에서는 빠진다.
        assert_eq!(restored.layouts.len(), 2);
        assert_eq!(restored.layouts[0].terminal_ids(), vec![3, 1]);
        assert_eq!(restored.layouts[1], pane(&[2], 0));
    }

    #[test]
    fn test_c_d38_broken_layouts_are_dropped_without_losing_work_or_selection() {
        let text = r#"{"version":2,"daemon":"7@t0",
            "work":{"folders":[],"terminals":[{"id":1,"folder":"/a","name":"x"}]},
            "groups":[],"selected_terminal":1,
            "layouts":[5,{"type":"pane","tabs":"no"},{"type":"pane","tabs":[1],"selected":0}],
            "sidebar_collapsed":"yes"}"#;
        let restored = SessionState::restore(text, &live(&[1]), "7@t0");
        assert_eq!(restored.layouts, vec![pane(&[1], 0)]);
        assert!(!restored.sidebar_collapsed, "a non-boolean collapse flag is ignored");
        assert_eq!(restored.work.terminals_of("/a"), vec![1]);
        assert_eq!(restored.selected, Some(1));
        let no_layouts =
            r#"{"version":2,"daemon":"7@t0","work":{"folders":[],"terminals":[]},"groups":[],"layouts":7}"#;
        assert!(SessionState::restore(no_layouts, &live(&[1]), "7@t0").layouts.is_empty());
    }

    #[test]
    fn test_c_d11b_v1_file_is_read_as_empty_groups() {
        // 슬라이스가 쓰던 평평한 터미널 목록. 폴더·묶음은 없고, 앱이 살아 있는 터미널을 새 묶음으로 데려간다.
        let v1 = r#"{"daemon":"7@t0","terminals":[{"id":1,"name":"a"},{"id":2,"name":"b"}],"selected":2}"#;
        let restored = SessionState::restore(v1, &live(&[1, 2]), "7@t0");
        assert!(restored.work.folders.is_empty());
        assert!(restored.work.groups.is_empty());
        assert_eq!(restored.daemon, "7@t0");
        assert_eq!(restored.selected, None);
    }

    #[test]
    fn test_c_d11b_malformed_group_dropped_without_losing_folders() {
        let text = r#"{"version":2,"daemon":"7@t0",
            "work":{"folders":[{"key":"/a","expanded":true},{"key":5},{"expanded":true}],"terminals":[{"id":1,"folder":"/a","name":"x"}]},
            "groups":[{"root":"x","members":[{"id":2,"name":"b"}]},{"root":3,"members":[{"id":3,"name":"c"},{"name":"d"}],"last_used":99},{"members":[]}],
            "selected_terminal":3}"#;
        let restored = SessionState::restore(text, &live(&[1, 2, 3]), "7@t0");
        assert_eq!(restored.work.folders.iter().map(|folder| folder.key.as_str()).collect::<Vec<_>>(), vec!["/a"]);
        assert_eq!(restored.work.terminals_of("/a"), vec![1]);
        // 모양이 틀린 묶음은 그 묶음만 버리고, 멤버 하나가 틀린 묶음은 그 멤버만 버린다.
        assert_eq!(restored.work.groups.len(), 1);
        assert_eq!(restored.work.group_members(3), vec![3]);
        // 없는 터미널을 가리키는 last_used는 묶음의 첫 멤버로 돌아간다.
        assert_eq!(restored.work.groups[0].last_used, 3);
        assert_eq!(restored.selected, Some(3));
    }

    #[test]
    fn test_c_d11b_broken_file_starts_empty() {
        assert_eq!(SessionState::restore("{not json", &live(&[1]), "7@t0"), SessionState::default());
        assert_eq!(SessionState::restore("", &live(&[1]), "7@t0"), SessionState::default());
        assert_eq!(SessionState::restore(r#"{"terminals": 3}"#, &live(&[1]), "7@t0"), SessionState::default());
        assert_eq!(SessionState::restore("[1, 2]", &live(&[1]), "7@t0"), SessionState::default());
        assert_eq!(
            SessionState::restore(r#"{"version":2,"daemon":"7@t0","work":3}"#, &live(&[1]), "7@t0").work,
            WorkState::default()
        );
    }

    #[test]
    fn test_c_d11b_unknown_terminal_dropped() {
        let restored = SessionState::restore(&sample().serialize(), &live(&[2, 3]), "7@t0");
        assert_eq!(restored.work.terminals_of("/home/me/app"), vec![3]);
        assert_eq!(restored.work.group_members(2), vec![2]);
        // 선택했던 터미널(1)이 없어졌으므로 선택된 것이 없다. 터미널이 없어진 폴더는 남는다.
        assert_eq!(restored.selected, None);
        assert_eq!(restored.work.folders.len(), 2);
    }

    #[test]
    fn test_c_d11b_malformed_entries_and_duplicates_dropped_without_losing_the_rest() {
        let text = r#"{"version":2,"daemon":"7@t0","work":{"folders":[],"terminals":[{"id":1,"folder":"/a","name":"a"},{"id":"x","folder":"/a","name":"b"},{"folder":"/a","name":"c"},{"id":2,"folder":"/a"},{"id":1,"folder":"/a","name":"dup"},{"id":4,"folder":"/a","name":"d"}]},"groups":[],"selected_terminal":4}"#;
        let restored = SessionState::restore(text, &live(&[1, 2, 4]), "7@t0");
        assert_eq!(restored.work.terminals_of("/a"), vec![1, 4]);
        assert_eq!(restored.selected, Some(4));
    }

    #[test]
    fn test_state_saved_by_another_daemon_keeps_the_folder_list_but_never_matches_terminals_by_id() {
        // 데몬이 죽고 새 데몬이 뜨면 번호가 1부터 다시 매겨져 저장된 번호가 다른 터미널을 가리킨다. 터미널 소속·선택·배치는 잇지 않는다.
        let mut saved = sample();
        saved.layouts = vec![pane(&[3, 1], 0)];
        saved.sidebar_collapsed = true;
        let restored = SessionState::restore(&saved.serialize(), &live(&[1, 2, 3]), "8@t1");
        assert_eq!(restored.daemon, "");
        assert_eq!(restored.selected, None);
        assert!(restored.layouts.is_empty());
        assert!(restored.work.terminals.is_empty() && restored.work.groups.is_empty());
        // 작업 폴더 목록(펼침·표시 이름·빈 폴더)과 사이드바 접힘은 데몬과 무관하게 남는다.
        assert_eq!(restored.work.folders, saved.work.folders);
        assert!(restored.sidebar_collapsed);
        // 데몬 표시가 없던 옛 파일(버전 1)은 읽지 않는다.
        let legacy = r#"{"terminals":[{"id":1,"name":"a"}],"selected":1}"#;
        assert_eq!(SessionState::restore(legacy, &live(&[1]), "7@t0"), SessionState::default());
    }

    #[test]
    fn test_c_d11b_a_state_saved_with_no_tabs_still_brings_back_the_folders() {
        // 탭을 모두 닫고 종료하면 저장되는 데몬 표시는 빈 글자다. 그래도 작업 폴더 목록은 다음 실행에 돌아온다.
        let mut empty = SessionState::default();
        empty.work.ensure_folder("/home/me/app");
        empty.work.set_folder_label("/home/me/app", "앱");
        let restored = SessionState::restore(&empty.serialize(), &HashSet::new(), "9@t2");
        assert_eq!(restored.work.folders, empty.work.folders);
    }
}
