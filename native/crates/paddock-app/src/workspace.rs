//! 창이 보여 주는 터미널 묶음: 탭, 작업 폴더·Unassigned 묶음 소속, 선택한 터미널, 새 터미널 이름, 저장할 상태.
//!
//! 화면과 데몬 연결을 모르는 순서·선택 규칙만 둔다. 탭이 쥐는 내용(`T`)은 호출자가 정한다.
//! 소속·이름 규칙(폴더 승격, 이름 번호, 탭 이동, 닫은 뒤 다음 선택)은 core의 `work_model`을 쓴다.
//!
//! 탭 줄과 본문은 선택한 터미널이 속한 묶음(작업 폴더 하나 또는 Unassigned 묶음 하나)의 터미널만 보여 준다.
//! 다른 묶음의 터미널도 탭으로 남아 계속 출력을 받는다.
//!
//! 묶음마다 터미널 배치 오른쪽에 파일 칸(`FilePanes`)이 하나 붙을 수 있다. 파일 칸은 터미널 번호가 아니라 묶음 이름표로 묶음을 찾으므로,
//! 묶음이 폴더로 올라가거나 시작 터미널이 닫혀 이름표가 바뀌는 연산(`add`·`close`·`promote`·`remove_folder`)이 칸을 새 이름표로 옮긴다.
//! `selected`는 "마지막으로 고른 터미널"이고 파일 탭이 활성인지는 창이 따로 쥔다.
//!
//! 묶음의 본문은 칸 배치(`Layout`)가 정한다. 배치는 터미널 번호가 어느 묶음에 있는지를 모르고 번호만 쥐며, 묶음 화면은
//! 저장한 배치들에서 그 묶음의 터미널만 남겨 이어 붙인 뒤 배치에 없는 터미널을 첫 칸 끝에 더해 만든다. 그래서 터미널이
//! 닫히거나 묶음이 폴더로 올라가도 배치가 어긋나지 않는다.
use std::collections::HashSet;

use paddock_core::file_kind::FileKind;
use paddock_core::file_pane::{FilePane, FilePanes, file_root_for};
use paddock_core::layout_model::{Layout, Orientation};
use paddock_core::pane_columns::tab_columns;
use paddock_core::session_state::SessionState;
use paddock_core::work_model::{Candidate, GroupKey, NewPlace, Promotion, WorkState};

/// 탭 줄 순서를 정할 때 쓰는 본문의 가상 크기. 칸 크기가 0이 아닌 한 열 순서는 크기와 무관하다.
const NOMINAL_BODY: f32 = 1000.0;

/// 탭 줄의 탭 하나: 터미널 또는 지금 보는 묶음 파일 칸의 N번째 파일 탭.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabRef {
    Terminal(u32),
    File(usize),
}

/// 탭 하나. 이름은 소속을 쥔 `WorkState`가 가진다(`Workspace::name`).
pub struct Tab<T> {
    pub id: u32,
    /// 이 터미널을 쥔 데몬의 식별 글자. 터미널 `id`는 데몬마다 1부터 매겨져, `id`는 이 글자와 함께여야 터미널을 가리킨다.
    pub daemon: String,
    pub content: T,
}

/// 탭 순서, 작업 목록의 소속, 선택한 터미널.
pub struct Workspace<T> {
    tabs: Vec<Tab<T>>,
    selected: Option<u32>,
    work: WorkState,
    /// 저장한 칸 배치들. 한 터미널은 한 배치에만 들고, 다 닫힌 터미널은 읽을 때 걸러진다.
    layouts: Vec<Layout>,
    sidebar_collapsed: bool,
    /// 묶음마다의 파일 칸.
    files: FilePanes,
}

impl<T> Default for Workspace<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            selected: None,
            work: WorkState::default(),
            layouts: Vec::new(),
            sidebar_collapsed: false,
            files: FilePanes::default(),
        }
    }
}

impl<T> Workspace<T> {
    pub fn tabs(&self) -> &[Tab<T>] {
        &self.tabs
    }

    pub fn selected(&self) -> Option<u32> {
        self.selected
    }

    pub fn work(&self) -> &WorkState {
        &self.work
    }

    /// 폴더 펼침·표시 이름처럼 탭을 건드리지 않는 소속 편집에 쓴다.
    pub fn work_mut(&mut self) -> &mut WorkState {
        &mut self.work
    }

    pub fn tab(
        &self,
        id: u32,
    ) -> Option<&Tab<T>> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    /// 목록에 보이는 터미널 이름. 소속이 없는 터미널은 기본 이름이다.
    pub fn name(
        &self,
        id: u32,
    ) -> &str {
        self.work.name_of(id).unwrap_or("terminal")
    }

    /// 저장된 소속을 되살린다. 탭을 붙이기 전에 한 번 부른다.
    pub fn set_work(
        &mut self,
        work: WorkState,
    ) {
        self.work = work;
    }

    /// 소속을 바꾸지 않고 탭만 맨 뒤에 붙인다. 저장된 소속을 되살릴 때 쓴다(선택도 그대로다).
    pub fn attach(
        &mut self,
        daemon: &str,
        id: u32,
        content: T,
    ) {
        self.tabs.push(Tab { id, daemon: daemon.to_string(), content });
    }

    /// 맨 뒤에 탭을 더하고 `place`에 소속시켜 선택한다. `daemon`은 그 터미널을 쥔 데몬의 식별 글자다.
    ///
    /// 다른 데몬의 탭이 있으면 모두 뺀다. 데몬이 죽고 새 데몬이 뜨면 터미널 번호가 1부터 다시 매겨져 죽은 탭의 번호와
    /// 겹치는데, 죽은 데몬의 탭은 어차피 다시 쓸 수 없고 같은 번호가 두 탭에 있으면 선택·닫기가 엉뚱한 탭에 가기 때문이다.
    /// 뺀 탭을 돌려준다(없으면 빈 목록). 새 터미널은 같은 묶음에서 먼저 선택돼 있던 터미널의 칸에 탭으로 들어간다.
    pub fn add(
        &mut self,
        daemon: &str,
        id: u32,
        name: &str,
        place: &NewPlace,
        content: T,
    ) -> Vec<Tab<T>> {
        self.add_arranged(daemon, id, name, place, content, None)
    }

    /// `add`와 같되 새 터미널을 먼저 선택돼 있던 터미널의 `orientation` 쪽(가로=옆, 세로=아래) 새 칸에 둔다.
    /// 같은 묶음에 선택돼 있던 터미널이 없으면 탭으로 들어간다.
    pub fn add_split(
        &mut self,
        daemon: &str,
        id: u32,
        name: &str,
        place: &NewPlace,
        content: T,
        orientation: Orientation,
    ) -> Vec<Tab<T>> {
        self.add_arranged(daemon, id, name, place, content, Some(orientation))
    }

    fn add_arranged(
        &mut self,
        daemon: &str,
        id: u32,
        name: &str,
        place: &NewPlace,
        content: T,
        split: Option<Orientation>,
    ) -> Vec<Tab<T>> {
        let anchor = self.selected;
        let work_before = self.work.clone();
        let (kept, evicted): (Vec<Tab<T>>, Vec<Tab<T>>) =
            std::mem::take(&mut self.tabs).into_iter().partition(|tab| tab.daemon == daemon);
        self.tabs = kept;
        for tab in &evicted {
            self.work.forget_terminal(tab.id);
        }
        // 쫓겨난 탭의 묶음 칸을 먼저 닫는다. 새 데몬이 같은 번호를 다시 매기므로, 그대로 두면 옛 묶음의 칸이 같은 번호의 새 묶음에 붙는다.
        self.files.follow_groups(&work_before, &self.work);
        let work_before = self.work.clone();
        match place {
            NewPlace::Folder(key) => self.work.assign_terminal(id, key, name),
            NewPlace::Group(root) => self.work.join_group(*root, id, name),
            NewPlace::NewGroup => self.work.add_group(id, name),
        }
        self.tabs.push(Tab { id, daemon: daemon.to_string(), content });
        self.selected = Some(id);
        self.files.follow_groups(&work_before, &self.work);
        // 묶음의 기존 화면에서 새 터미널을 뺀 배치를 만들고, 앵커 칸에 탭으로 더하거나 옆·아래 칸으로 나눈다.
        let ids = self.group_ids(id);
        let others: Vec<u32> = ids.iter().copied().filter(|other| *other != id).collect();
        let base = self.layout_for(&others);
        let anchor = anchor.filter(|anchor| others.contains(anchor));
        let arranged = match (base, anchor, split) {
            (Some(base), Some(anchor), Some(orientation)) => base.split(anchor, id, orientation),
            (Some(base), Some(anchor), None) => base.add_beside(anchor, id),
            (Some(base), None, _) => Layout::with_terminal(Some(&base), id),
            (None, _, _) => Layout::with_terminal(None, id),
        };
        self.commit_layout(&ids, arranged.select(id));
        evicted
    }

    /// 선택을 바꾼다. 없는 id면 아무것도 바꾸지 않고 false다. 묶음에서 마지막으로 쓴 터미널로 기록한다.
    pub fn select(
        &mut self,
        id: u32,
    ) -> bool {
        let found = self.tab(id).is_some();
        if found {
            self.set_selected(id);
            self.work.touch(id);
        }
        found
    }

    /// 선택을 정한다. 저장된 선택을 되살릴 때 쓰며, 없는 id거나 None이면 첫 탭을 고른다.
    pub fn select_or_first(
        &mut self,
        id: Option<u32>,
    ) {
        let chosen = id.filter(|id| self.tab(*id).is_some()).or_else(|| self.tabs.first().map(|tab| tab.id));
        match chosen {
            Some(chosen) => self.set_selected(chosen),
            None => self.selected = None,
        }
    }

    /// 선택한 터미널이 속한 묶음. 선택한 터미널이 없으면 None이다.
    pub fn view_key(&self) -> Option<GroupKey> {
        self.selected.and_then(|id| self.work.group_key_of(id))
    }

    /// 지금 탭 줄이 보여 주는 터미널(선택한 터미널의 묶음). 탭 줄에 놓인 순서이며, 칸이 나뉘어 있으면 왼쪽 열의 칸부터
    /// 위에서 아래 순서로 각 칸의 탭이 이어진다.
    pub fn view_ids(&self) -> Vec<u32> {
        match self.view_layout() {
            Some(layout) => {
                tab_columns(&layout, NOMINAL_BODY, NOMINAL_BODY).into_iter().flat_map(|column| column.tabs).collect()
            }
            None => Vec::new(),
        }
    }

    /// 지금 보는 묶음의 칸 배치. 선택한 터미널은 자기 칸의 선택된 탭이다. 선택한 터미널이 없으면 None이다.
    pub fn view_layout(&self) -> Option<Layout> {
        let selected = self.selected?;
        self.layout_for(&self.group_ids(selected)).map(|layout| layout.select(selected))
    }

    /// 터미널이 속한 묶음의 터미널(탭이 있는 것만, 묶음 순서).
    fn group_ids(
        &self,
        id: u32,
    ) -> Vec<u32> {
        self.work
            .group_key_of(id)
            .map(|key| self.work.ids_in(&key))
            .unwrap_or_default()
            .into_iter()
            .filter(|other| self.tab(*other).is_some())
            .collect()
    }

    /// `ids`(한 묶음의 터미널)의 화면 배치. 저장한 배치들에서 `ids`만 남겨 이어 붙이고, 배치에 없는 터미널은 첫 칸 끝에 더한다.
    /// `ids`가 비면 None이다.
    fn layout_for(
        &self,
        ids: &[u32],
    ) -> Option<Layout> {
        let open: HashSet<u32> = ids.iter().copied().collect();
        let mut layout = Layout::besides(self.layouts.iter().map(|saved| saved.without_closed(&open)).collect());
        for id in ids {
            if !layout.as_ref().is_some_and(|layout| layout.includes(*id)) {
                layout = Some(Layout::with_terminal(layout.as_ref(), *id));
            }
        }
        layout
    }

    /// 한 묶음(`ids`)의 배치를 `layout`으로 바꾼다. 다른 묶음의 터미널은 저장한 배치에 그대로 남는다.
    fn commit_layout(
        &mut self,
        ids: &[u32],
        layout: Layout,
    ) {
        let open: HashSet<u32> = ids.iter().copied().collect();
        let mut committed = Vec::new();
        let mut pending = Some(layout);
        for saved in std::mem::take(&mut self.layouts) {
            let touches_group = saved.terminal_ids().iter().any(|id| open.contains(id));
            if touches_group && let Some(layout) = pending.take() {
                committed.push(layout);
            }
            if let Some(rest) = saved.prune(|id| !open.contains(&id)) {
                committed.push(rest);
            }
        }
        committed.extend(pending);
        self.layouts = committed;
    }

    /// 선택을 `id`로 정하고 그 터미널을 자기 칸의 선택된 탭으로 만든다. `id`는 탭이 있는 터미널이어야 한다.
    fn set_selected(
        &mut self,
        id: u32,
    ) {
        self.selected = Some(id);
        let ids = self.group_ids(id);
        if let Some(layout) = self.layout_for(&ids) {
            self.commit_layout(&ids, layout.select(id));
        }
    }

    /// 지금 보는 묶음의 `path`(자식 번호의 열)가 가리키는 분할의 칸 크기 비율을 바꾼다. 경계를 끌어 크기를 바꿀 때 쓴다.
    /// 칸 수와 다른 비율 목록이나 없는 경로는 무시한다.
    pub fn resize_split(
        &mut self,
        path: &[usize],
        sizes: Vec<f64>,
    ) {
        if let Some(selected) = self.selected {
            let ids = self.group_ids(selected);
            if let Some(layout) = self.layout_for(&ids) {
                self.commit_layout(&ids, layout.with_sizes(path, sizes).select(selected));
            }
        }
    }

    /// 저장된 칸 배치를 되살린다. 탭을 붙인 뒤 선택을 정하기 전에 부른다.
    pub fn set_layouts(
        &mut self,
        layouts: Vec<Layout>,
    ) {
        self.layouts = layouts;
    }

    pub fn sidebar_collapsed(&self) -> bool {
        self.sidebar_collapsed
    }

    pub fn set_sidebar_collapsed(
        &mut self,
        collapsed: bool,
    ) {
        self.sidebar_collapsed = collapsed;
    }

    /// 새 터미널이 열릴 자리: 보이는 작업 폴더, 없으면 보이는 Unassigned 묶음, 둘 다 없으면 새 묶음.
    pub fn new_place(&self) -> NewPlace {
        let folder = self.selected.and_then(|id| self.work.folder_of(id));
        let group = self.selected.and_then(|id| self.work.group_of(id));
        paddock_core::work_model::new_terminal_place(folder, group)
    }

    /// 탭을 닫고 돌려준다. 선택한 탭을 닫았으면 같은 묶음에서 그 자리에 들어온 다음 탭을, 없으면 이전 탭을 선택하고,
    /// 묶음에 남은 탭이 없으면 사이드바 순서에서 가까운 묶음의 터미널을 선택한다. 다른 탭을 닫았으면 선택은 그대로다.
    ///
    /// Examples
    /// --------
    /// | 탭 줄     | 선택 | 닫는 탭 | 선택 결과 |
    /// |-----------|------|---------|-----------|
    /// | 1 2 3 4   | 3    | 3       | 4         |
    /// | 1 2 3     | 3    | 3       | 2         |
    /// | 1 2 3     | 1    | 3       | 1         |
    /// | 1 (묶음 A), 5 (묶음 B) | 1 | 1 | 5   |
    /// | 1         | 1    | 1       | 없음      |
    pub fn close(
        &mut self,
        id: u32,
    ) -> Option<Tab<T>> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let was_selected = self.selected == Some(id);
        let row = self.view_ids();
        let key = self.work.group_key_of(id);
        let fallback = key.as_ref().and_then(|key| self.work.nearest_after_close(key, id));
        let closed = self.tabs.remove(index);
        let work_before = self.work.clone();
        self.work.forget_terminal(id);
        self.files.follow_groups(&work_before, &self.work);
        if was_selected {
            let position = row.iter().position(|other| *other == id).unwrap_or(0);
            let remaining: Vec<u32> = row.iter().copied().filter(|other| *other != id).collect();
            let neighbor =
                remaining.get(position).or_else(|| position.checked_sub(1).and_then(|before| remaining.get(before)));
            let next = neighbor.copied().or(fallback).filter(|next| self.tab(*next).is_some());
            match next {
                Some(next) => self.set_selected(next),
                None => self.selected = None,
            }
        }
        Some(closed)
    }

    /// 새 터미널의 기본 이름(`terminal N`). 작업 폴더와 Unassigned 묶음 전체에서 쓰는 번호를 피한다.
    pub fn next_name(&self) -> String {
        self.work.next_terminal_name()
    }

    pub fn rename(
        &mut self,
        id: u32,
        name: &str,
    ) {
        self.work.rename_terminal(id, name);
    }

    /// 에이전트가 실행 중인 터미널을 작업 폴더로 올린다. 선택한 터미널이 올려져도 선택은 그대로이고 탭 줄만 새 폴더 것으로 바뀐다.
    pub fn promote(
        &mut self,
        candidates: &[Candidate],
    ) -> Promotion {
        let work_before = self.work.clone();
        let promotion = self.work.promote_agent_terminals(candidates);
        self.files.follow_groups(&work_before, &self.work);
        promotion
    }

    /// 작업 폴더를 목록에서 뺀다(그 폴더의 터미널 소속도 함께 지운다). 그 폴더의 파일 칸도 사라진다. 디스크의 폴더는 건드리지 않는다.
    pub fn remove_folder(
        &mut self,
        key: &str,
    ) {
        let work_before = self.work.clone();
        self.work.remove_folder(key, None);
        self.files.follow_groups(&work_before, &self.work);
    }

    /// 묶음마다의 파일 칸.
    pub fn files(&self) -> &FilePanes {
        &self.files
    }

    /// 저장된 파일 칸을 되살린다. 탭을 붙이기 전에 소속(`set_work`)을 되살린 뒤 부른다.
    pub fn set_files(
        &mut self,
        files: FilePanes,
    ) {
        self.files = files;
    }

    /// 지금 보는 묶음의 파일 칸. 파일 탭이 없으면 None이다.
    pub fn file_pane(&self) -> Option<&FilePane> {
        self.view_key().and_then(|key| self.files.get(&key))
    }

    /// 파일을 속할 묶음의 칸에 열고(이미 열려 있으면 그 탭을 선택하고) 그 묶음 이름표와 탭 번호를 돌려준다.
    /// 작업 폴더가 파일을 품으면 그 폴더, 아니면 지금 보는 묶음이 쥔다. 속할 묶음이 없으면(터미널이 없음) None이다.
    pub fn open_file(
        &mut self,
        path: &str,
        kind: FileKind,
    ) -> Option<(GroupKey, usize)> {
        let shown = self.view_key();
        let top = self.tabs.first().and_then(|tab| self.work.group_key_of(tab.id));
        let owner = file_root_for(&self.work, path, shown.as_ref(), top.as_ref())?;
        // 다른 묶음의 작업 폴더가 품은 파일이면 화면을 그 폴더 묶음으로 옮겨 보이게 한다. 보일 터미널이 없는 폴더면 지금 보는 묶음에 연다.
        let target = self.work.ids_in(&owner).into_iter().find(|id| self.tab(*id).is_some());
        let key = match (&shown, target) {
            (Some(shown), _) if *shown == owner => owner,
            (_, Some(id)) => {
                self.select(id);
                owner
            }
            (Some(shown), None) => shown.clone(),
            (None, None) => top.unwrap_or(owner),
        };
        let index = self.files.open(&key, path, kind);
        Some((key, index))
    }

    /// 파일 탭을 닫는다. 닫을 탭이 없으면 false다.
    pub fn close_file(
        &mut self,
        key: &GroupKey,
        index: usize,
    ) -> bool {
        self.files.close(key, index)
    }

    /// 파일 탭을 선택한다. 없는 탭이면 false다.
    pub fn select_file(
        &mut self,
        key: &GroupKey,
        index: usize,
    ) -> bool {
        self.files.select(key, index)
    }

    /// 파일 탭이 내용을 보여 주는 방식을 바꾼다(이진 안내 탭 → 텍스트).
    pub fn set_file_kind(
        &mut self,
        key: &GroupKey,
        index: usize,
        kind: FileKind,
    ) {
        self.files.set_kind(key, index, kind);
    }

    /// 파일 칸의 폭 비율을 바꾼다.
    pub fn set_file_ratio(
        &mut self,
        key: &GroupKey,
        ratio: f32,
    ) {
        self.files.set_ratio(key, ratio);
    }

    /// 지금 탭 줄이 보여 주는 탭의 순서: 터미널 탭 줄 순서(`view_ids`) 뒤에 파일 칸의 탭이 이어진다. 탭 이동 키가 이 순서로 돈다.
    pub fn tab_order(&self) -> Vec<TabRef> {
        let n_files = self.file_pane().map_or(0, |pane| pane.tabs.len());
        self.view_ids().into_iter().map(TabRef::Terminal).chain((0..n_files).map(TabRef::File)).collect()
    }

    /// state.json에 저장할 상태. 배치는 탭이 있는 터미널만 남긴다. 파일 목록 구획의 접힘(`files_collapsed`)은 창이 쥐므로 호출자가 채운다.
    pub fn to_state(&self) -> SessionState {
        let open: HashSet<u32> = self.tabs.iter().map(|tab| tab.id).collect();
        SessionState {
            daemon: self.tabs.first().map(|tab| tab.daemon.clone()).unwrap_or_default(),
            work: self.work.clone(),
            selected: self.selected,
            layouts: self.layouts.iter().filter_map(|layout| layout.without_closed(&open)).collect(),
            sidebar_collapsed: self.sidebar_collapsed,
            files: self.files.clone(),
            files_collapsed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use paddock_core::layout_model::{Layout, Orientation};
    use paddock_core::work_model::{Candidate, TabTarget, tab_target};

    use super::*;

    const DAEMON: &str = "10@t0";

    /// 모든 탭이 한 Unassigned 묶음에 든 작업 공간.
    fn workspace(ids: &[u32]) -> Workspace<()> {
        let mut workspace = Workspace::default();
        for id in ids {
            let name = workspace.next_name();
            let place = match ids.first() {
                Some(root) if root != id => NewPlace::Group(*root),
                _ => NewPlace::NewGroup,
            };
            workspace.add(DAEMON, *id, &name, &place, ());
        }
        workspace
    }

    /// 터미널 탭만 있는 줄에서 `target`으로 선택을 옮기는 시험 보조(창이 하는 `tab_order` + `tab_target`과 같다).
    fn step(
        workspace: &mut Workspace<()>,
        target: TabTarget,
    ) -> Option<u32> {
        let order = workspace.tab_order();
        let current = workspace.selected().map(TabRef::Terminal)?;
        let next = tab_target(&order, &current, target).copied();
        if let Some(TabRef::Terminal(id)) = next {
            workspace.select(id);
            Some(id)
        } else {
            None
        }
    }

    fn ids(workspace: &Workspace<()>) -> Vec<u32> {
        workspace.tabs().iter().map(|tab| tab.id).collect()
    }

    #[test]
    fn test_add_appends_selects_and_numbers_names_without_reuse() {
        let mut workspace = workspace(&[7, 3]);
        assert_eq!(ids(&workspace), vec![7, 3]);
        assert_eq!(workspace.selected(), Some(3));
        assert_eq!(workspace.name(3), "terminal 2");
        // 앞 탭을 닫아도 남은 탭 이름은 바뀌지 않고, 빈 번호가 다시 쓰인다.
        workspace.close(7);
        assert_eq!(workspace.name(3), "terminal 2");
        assert_eq!(workspace.next_name(), "terminal 1");
    }

    #[test]
    fn test_close_selected_picks_the_next_tab_then_the_previous_and_close_other_keeps_selection() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.select(2);
        workspace.close(3);
        assert_eq!(workspace.selected(), Some(2));
        let closed = workspace.close(2).unwrap();
        assert_eq!(closed.id, 2);
        assert_eq!(workspace.selected(), Some(1));
        workspace.close(1);
        assert_eq!(workspace.selected(), None);
        assert!(workspace.close(1).is_none());
    }

    #[test]
    fn test_close_selected_goes_to_the_tab_that_took_its_place() {
        // 탭 줄에서 닫은 자리에 다음 탭이 들어온다. 그 탭이 선택된다(맨 앞 탭이 아니다).
        let mut workspace = workspace(&[1, 2, 3, 4]);
        workspace.select(3);
        workspace.close(3);
        assert_eq!(workspace.selected(), Some(4));
        workspace.select(2);
        workspace.close(2);
        assert_eq!(workspace.selected(), Some(4));
        // 맨 뒤 탭을 닫으면 이전 탭이다.
        workspace.close(4);
        assert_eq!(workspace.selected(), Some(1));
        // 선택하지 않은 탭을 닫아도 선택은 그대로다.
        let mut other = self::workspace(&[1, 2, 3]);
        other.select(1);
        other.close(3);
        assert_eq!(other.selected(), Some(1));
    }

    #[test]
    fn test_add_from_a_new_daemon_drops_the_tabs_of_the_dead_one_so_ids_never_collide() {
        let mut workspace = workspace(&[1, 2]);
        // 데몬이 죽고 새 데몬이 뜨면 번호가 1부터 다시 매겨져 죽은 탭의 번호와 겹친다.
        let evicted = workspace.add("20@t1", 1, "terminal 1", &NewPlace::NewGroup, ());
        assert_eq!(evicted.iter().map(|tab| tab.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(ids(&workspace), vec![1]);
        assert_eq!(workspace.selected(), Some(1));
        assert_eq!(workspace.work().groups.len(), 1, "the dead daemon's groups are forgotten too");
        // 같은 데몬의 터미널은 그대로 쌓인다.
        assert!(workspace.add("20@t1", 2, "terminal 2", &NewPlace::Group(1), ()).is_empty());
        assert_eq!(ids(&workspace), vec![1, 2]);
    }

    #[test]
    fn test_step_wraps_around_and_does_nothing_without_tabs() {
        let mut workspace = workspace(&[1, 2, 3]);
        assert_eq!(step(&mut workspace, TabTarget::Next), Some(1));
        assert_eq!(step(&mut workspace, TabTarget::Previous), Some(3));
        assert_eq!(step(&mut workspace, TabTarget::First), Some(1));
        assert_eq!(step(&mut workspace, TabTarget::Last), Some(3));
        let mut empty: Workspace<()> = Workspace::default();
        assert_eq!(step(&mut empty, TabTarget::Next), None);
    }

    #[test]
    fn test_select_ignores_unknown_ids_and_restore_falls_back_to_first_tab() {
        let mut workspace = workspace(&[4, 5]);
        assert!(!workspace.select(99));
        assert_eq!(workspace.selected(), Some(5));
        workspace.select_or_first(Some(4));
        assert_eq!(workspace.selected(), Some(4));
        workspace.select_or_first(Some(99));
        assert_eq!(workspace.selected(), Some(4));
        workspace.select_or_first(None);
        assert_eq!(workspace.selected(), Some(4));
    }

    #[test]
    fn test_to_state_keeps_membership_names_selection_and_the_daemon() {
        let mut workspace = workspace(&[9, 2]);
        workspace.select(9);
        let state = workspace.to_state();
        assert_eq!(state.daemon, DAEMON);
        assert_eq!(state.terminal_ids(), vec![9, 2]);
        assert_eq!(state.work.name_of(2), Some("terminal 2"));
        assert_eq!(state.work.groups[0].last_used, 9);
        assert_eq!(state.selected, Some(9));
        assert_eq!(Workspace::<()>::default().to_state().daemon, "");
    }

    /// 작업 폴더 하나(터미널 1·2)와 Unassigned 묶음 하나(터미널 3·4)가 있는 작업 공간.
    fn two_groups() -> Workspace<()> {
        let mut workspace = Workspace::default();
        workspace.add(DAEMON, 1, "claude", &NewPlace::Folder("/repo".to_string()), ());
        workspace.add(DAEMON, 2, "terminal 2", &NewPlace::Folder("/repo".to_string()), ());
        workspace.add(DAEMON, 3, "terminal 3", &NewPlace::NewGroup, ());
        workspace.add(DAEMON, 4, "terminal 4", &NewPlace::Group(3), ());
        workspace
    }

    #[test]
    fn test_c_d38_switching_groups_hides_other_tabs_and_keeps_terminals() {
        let mut workspace = two_groups();
        // 방금 연 터미널 4가 선택돼 있고 탭 줄은 그 묶음(3·4)만 보인다. 다른 묶음의 터미널도 탭으로 남아 있다.
        assert_eq!(workspace.selected(), Some(4));
        assert_eq!(workspace.view_ids(), vec![3, 4]);
        assert_eq!(ids(&workspace), vec![1, 2, 3, 4]);
        // 폴더 쪽 터미널을 고르면 탭 줄이 폴더의 터미널로 바뀌고 앞 묶음의 터미널은 그대로 남는다.
        assert!(workspace.select(2));
        assert_eq!(workspace.view_ids(), vec![1, 2]);
        assert_eq!(workspace.tabs().len(), 4);
        // 탭 이동은 보이는 탭 줄 안에서만 돈다.
        assert_eq!(step(&mut workspace, TabTarget::Next), Some(1));
        assert_eq!(step(&mut workspace, TabTarget::Previous), Some(2));
        // 묶음으로 돌아오면 그 묶음에서 마지막으로 쓴 터미널로 이어진다.
        workspace.select(3);
        workspace.select(2);
        assert_eq!(workspace.work().groups[0].last_used, 3);
    }

    #[test]
    fn test_closing_the_last_tab_of_a_group_selects_the_nearest_group_at_its_last_used_terminal() {
        let mut workspace = two_groups();
        workspace.select(4);
        workspace.select(3);
        workspace.close(3);
        // 묶음에 4가 남아 같은 묶음에 머문다.
        assert_eq!(workspace.selected(), Some(4));
        workspace.close(4);
        // Unassigned 묶음이 비면 사이드바에서 가까운 폴더 묶음이다.
        assert_eq!(workspace.selected(), Some(1));
        assert_eq!(workspace.view_ids(), vec![1, 2]);
    }

    #[test]
    fn test_new_place_follows_the_shown_group() {
        let mut workspace = two_groups();
        assert_eq!(workspace.new_place(), NewPlace::Group(3));
        workspace.select(1);
        assert_eq!(workspace.new_place(), NewPlace::Folder("/repo".to_string()));
        assert_eq!(Workspace::<()>::default().new_place(), NewPlace::NewGroup);
    }

    #[test]
    fn test_promoting_the_shown_group_keeps_the_selection_and_moves_the_tab_row_to_the_folder() {
        let mut workspace = two_groups();
        let candidates = vec![Candidate {
            id: 4,
            cwd: Some("/work/new".to_string()),
            program: Some("claude".to_string()),
            is_agent: true,
            name: "terminal 4".to_string(),
        }];
        let promotion = workspace.promote(&candidates);
        assert_eq!(promotion.promoted, vec![3, 4]);
        assert_eq!(workspace.selected(), Some(4));
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/work/new".to_string())));
        assert_eq!(workspace.view_ids(), vec![3, 4]);
        assert_eq!(workspace.name(4), "claude");
    }

    #[test]
    fn test_c_d51_opening_the_same_file_twice_selects_the_existing_tab() {
        let mut workspace = two_groups();
        let first = workspace.open_file("/tmp/a.txt", FileKind::Text);
        let second = workspace.open_file("/tmp/b.txt", FileKind::Text);
        assert_eq!((first.clone(), second), (Some((GroupKey::Group(3), 0)), Some((GroupKey::Group(3), 1))));
        assert_eq!(workspace.open_file("/tmp/a.txt", FileKind::Text), first);
        let pane = workspace.file_pane().unwrap();
        assert_eq!((pane.tabs.len(), pane.selected), (2, 0));
        // 작업 폴더가 품은 파일은 지금 어느 묶음을 보든 그 폴더의 칸에 열린다.
        assert_eq!(workspace.open_file("/repo/src/x.rs", FileKind::Text), Some((GroupKey::Folder("/repo".into()), 0)));
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/repo".into())), "the folder's group is shown now");
    }

    #[test]
    fn test_c_d38_switching_groups_hides_other_files() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.select(1);
        workspace.open_file("/repo/readme.md", FileKind::Markdown);
        let shown = |workspace: &Workspace<()>| -> Vec<String> {
            workspace.file_pane().map(|pane| pane.tabs.iter().map(|tab| tab.path.clone()).collect()).unwrap_or_default()
        };
        assert_eq!(shown(&workspace), vec!["/repo/readme.md"]);
        workspace.select(4);
        assert_eq!(shown(&workspace), vec!["/tmp/a.txt"]);
        workspace.select(2);
        assert_eq!(shown(&workspace), vec!["/repo/readme.md"], "coming back shows the same tabs");
        assert_eq!(workspace.files().len(), 2);
    }

    #[test]
    fn test_c_d51_files_follow_their_group_when_it_is_promoted() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.set_file_ratio(&GroupKey::Group(3), 0.3);
        let candidates = vec![Candidate {
            id: 4,
            cwd: Some("/work/new".to_string()),
            program: Some("claude".to_string()),
            is_agent: true,
            name: "terminal 4".to_string(),
        }];
        workspace.promote(&candidates);
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/work/new".to_string())));
        let pane = workspace.file_pane().unwrap();
        assert_eq!((pane.tabs.len(), pane.ratio), (1, 0.3));
        assert!(workspace.files().get(&GroupKey::Group(3)).is_none());
    }

    #[test]
    fn test_c_d51_file_pane_follows_group_root_change() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        // 묶음 [3, 4]의 시작 터미널 3이 닫혀 4가 시작 터미널이 되어도 파일 칸이 따라온다.
        workspace.close(3);
        assert_eq!(workspace.view_key(), Some(GroupKey::Group(4)));
        assert_eq!(workspace.file_pane().map(|pane| pane.tabs.len()), Some(1));
        assert!(workspace.files().get(&GroupKey::Group(3)).is_none());
    }

    #[test]
    fn test_c_d51_closing_the_last_terminal_of_a_group_closes_its_files() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.open_file("/tmp/b.txt", FileKind::Text);
        workspace.open_file("/repo/readme.md", FileKind::Markdown);
        workspace.close(3);
        workspace.close(4);
        assert_eq!(workspace.files().len(), 1, "only the folder pane is left");
        assert!(workspace.files().get(&GroupKey::Folder("/repo".into())).is_some());
        // 폴더는 터미널이 모두 닫혀도 파일 칸이 남는다.
        workspace.close(1);
        workspace.close(2);
        assert!(workspace.files().get(&GroupKey::Folder("/repo".into())).is_some());
    }

    #[test]
    fn test_c_d51_removing_a_folder_from_the_list_drops_its_files_and_the_state_keeps_the_rest() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.open_file("/repo/readme.md", FileKind::Markdown);
        let state = workspace.to_state();
        assert_eq!(state.files.len(), 2);
        workspace.remove_folder("/repo");
        assert!(workspace.files().get(&GroupKey::Folder("/repo".into())).is_none());
        assert_eq!(workspace.to_state().files.len(), 1);
    }

    #[test]
    fn test_c_d51_stepping_continues_from_the_last_terminal_into_the_file_tabs_and_back() {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.open_file("/tmp/b.txt", FileKind::Text);
        workspace.select(3);
        let order = workspace.tab_order();
        assert_eq!(
            order,
            vec![TabRef::Terminal(3), TabRef::Terminal(4), TabRef::File(0), TabRef::File(1)],
            "the file tabs are the last column of the strip"
        );
    }

    #[test]
    fn test_c_d51_opening_a_folder_file_from_another_group_switches_to_that_folders_group() {
        let mut workspace = two_groups();
        assert_eq!(workspace.view_key(), Some(GroupKey::Group(3)));
        let opened = workspace.open_file("/repo/src/x.rs", FileKind::Text);
        assert_eq!(opened, Some((GroupKey::Folder("/repo".into()), 0)));
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/repo".into())), "the folder's group is now shown");
        assert_eq!(workspace.file_pane().map(|pane| pane.tabs.len()), Some(1));
    }

    #[test]
    fn test_c_d51_a_folder_file_opens_in_the_shown_group_when_the_folder_has_no_terminal() {
        let mut workspace = two_groups();
        workspace.close(1);
        workspace.close(2);
        workspace.select(4);
        let opened = workspace.open_file("/repo/src/x.rs", FileKind::Text);
        assert_eq!(opened, Some((GroupKey::Group(3), 0)), "a group that cannot be shown is not used");
        assert_eq!(workspace.view_key(), Some(GroupKey::Group(3)));
    }

    #[test]
    fn test_c_d51_files_of_a_dead_daemons_group_do_not_attach_to_a_new_group_with_the_same_root() {
        let mut workspace = workspace(&[1, 2]);
        workspace.open_file("/tmp/old-session.txt", FileKind::Text);
        // 데몬이 바뀌어 번호가 1부터 다시 매겨진다: 새 터미널 1이 새 묶음의 시작 터미널이 된다.
        workspace.add("20@t1", 1, "terminal 1", &NewPlace::NewGroup, ());
        assert!(workspace.files().is_empty(), "the old daemon's files are closed with their group");
        assert!(workspace.file_pane().is_none());
    }

    fn pane(
        tabs: &[u32],
        selected: usize,
    ) -> Layout {
        Layout::pane(tabs.to_vec(), selected)
    }

    fn two_panes(
        orientation: Orientation,
        first: Layout,
        second: Layout,
    ) -> Layout {
        Layout::Split { orientation, children: vec![first, second], sizes: vec![0.5, 0.5] }
    }

    #[test]
    fn test_c_d38_a_group_without_a_saved_layout_is_one_pane_with_all_its_tabs_in_order() {
        let mut workspace = workspace(&[1, 2, 3]);
        assert_eq!(workspace.view_layout(), Some(pane(&[1, 2, 3], 2)));
        workspace.select(1);
        assert_eq!(workspace.view_layout(), Some(pane(&[1, 2, 3], 0)));
        assert_eq!(Workspace::<()>::default().view_layout(), None);
    }

    #[test]
    fn test_c_d38_split_right_opens_the_new_terminal_in_its_own_pane_beside_the_selected_one() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(1);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        assert_eq!(workspace.selected(), Some(3));
        assert_eq!(workspace.view_layout(), Some(two_panes(Orientation::Horizontal, pane(&[1, 2], 0), pane(&[3], 0))));
        assert_eq!(workspace.view_ids(), vec![1, 2, 3]);
    }

    #[test]
    fn test_c_d38_split_down_stacks_the_new_pane_under_the_selected_pane() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(2);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Vertical);
        assert_eq!(workspace.view_layout(), Some(two_panes(Orientation::Vertical, pane(&[1, 2], 1), pane(&[3], 0))));
    }

    #[test]
    fn test_c_d38_a_plain_new_terminal_joins_the_pane_of_the_selected_one_as_a_tab() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(1);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        workspace.select(3);
        workspace.add(DAEMON, 4, "terminal 4", &NewPlace::Group(1), ());
        assert_eq!(
            workspace.view_layout(),
            Some(two_panes(Orientation::Horizontal, pane(&[1, 2], 0), pane(&[3, 4], 1)))
        );
        // 탭 줄 순서는 칸에 놓인 순서다.
        assert_eq!(workspace.view_ids(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_c_d38_selecting_a_tab_changes_only_the_selected_tab_of_its_own_pane() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(1);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        workspace.select(2);
        assert_eq!(workspace.view_layout(), Some(two_panes(Orientation::Horizontal, pane(&[1, 2], 1), pane(&[3], 0))));
        workspace.select(3);
        workspace.select(1);
        assert_eq!(workspace.view_layout(), Some(two_panes(Orientation::Horizontal, pane(&[1, 2], 0), pane(&[3], 0))));
    }

    #[test]
    fn test_c_d38_each_group_keeps_its_own_layout_and_ratio_across_switches() {
        let mut workspace = two_groups();
        // 묶음 B(3·4)를 옆으로 나누고 비율을 바꾼다.
        workspace.select(3);
        workspace.add_split(DAEMON, 5, "terminal 5", &NewPlace::Group(3), (), Orientation::Horizontal);
        workspace.resize_split(&[], vec![3.0, 1.0]);
        let split_layout = workspace.view_layout().unwrap();
        // 폴더 쪽으로 갔다 돌아오면 같은 배치이고, 폴더의 화면은 칸 하나다.
        workspace.select(1);
        assert_eq!(workspace.view_layout(), Some(pane(&[1, 2], 0)));
        assert_eq!(workspace.view_ids(), vec![1, 2]);
        workspace.select(3);
        assert_eq!(workspace.view_layout(), Some(split_layout));
        // 다른 묶음의 터미널은 이 묶음 화면에 섞이지 않는다.
        assert!(!workspace.view_layout().unwrap().includes(1));
        assert_eq!(workspace.tabs().len(), 5, "no terminal ended");
    }

    #[test]
    fn test_c_d38_closing_the_last_tab_of_a_pane_removes_the_pane_and_collapses_the_split() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(1);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        workspace.close(3);
        // 닫은 탭이 줄 끝이었으므로 이전 탭(2)이 선택되고, 그 칸의 선택 탭도 2다.
        assert_eq!(workspace.selected(), Some(2));
        assert_eq!(workspace.view_layout(), Some(pane(&[1, 2], 1)));
    }

    #[test]
    fn test_c_d38_promoting_a_split_group_to_a_folder_keeps_its_layout() {
        let mut workspace = two_groups();
        workspace.select(3);
        workspace.add_split(DAEMON, 5, "terminal 5", &NewPlace::Group(3), (), Orientation::Vertical);
        let before = workspace.view_layout().unwrap();
        let candidates = vec![Candidate {
            id: 4,
            cwd: Some("/work/new".to_string()),
            program: Some("claude".to_string()),
            is_agent: true,
            name: "terminal 4".to_string(),
        }];
        workspace.promote(&candidates);
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/work/new".to_string())));
        assert_eq!(workspace.view_layout(), Some(before));
    }

    #[test]
    fn test_c_d38_step_follows_the_tab_strip_order_across_panes() {
        let mut workspace = workspace(&[1, 2]);
        workspace.select(1);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        workspace.select(1);
        assert_eq!(step(&mut workspace, TabTarget::Next), Some(2));
        assert_eq!(step(&mut workspace, TabTarget::Next), Some(3));
        assert_eq!(step(&mut workspace, TabTarget::Next), Some(1));
        assert_eq!(step(&mut workspace, TabTarget::Last), Some(3));
        assert_eq!(step(&mut workspace, TabTarget::First), Some(1));
    }

    #[test]
    fn test_c_d38_layouts_and_sidebar_collapse_survive_the_saved_state() {
        let mut workspace = two_groups();
        workspace.select(3);
        workspace.add_split(DAEMON, 5, "terminal 5", &NewPlace::Group(3), (), Orientation::Horizontal);
        workspace.resize_split(&[], vec![2.0, 1.0]);
        workspace.set_sidebar_collapsed(true);
        let state = workspace.to_state();
        assert!(state.sidebar_collapsed);
        let live: std::collections::HashSet<u32> = (1..=5).collect();
        let restored = SessionState::restore(&state.serialize(), &live, DAEMON);
        let mut again = Workspace::<()>::default();
        again.set_work(restored.work.clone());
        for id in 1..=5 {
            again.attach(DAEMON, id, ());
        }
        again.set_layouts(restored.layouts.clone());
        again.set_sidebar_collapsed(restored.sidebar_collapsed);
        again.select_or_first(restored.selected);
        assert_eq!(again.selected(), Some(5));
        assert_eq!(again.view_layout(), workspace.view_layout());
        assert!(again.sidebar_collapsed());
    }

    #[test]
    fn test_c_d38_resize_split_ignores_a_size_list_of_the_wrong_length() {
        let mut workspace = workspace(&[1]);
        workspace.add_split(DAEMON, 2, "terminal 2", &NewPlace::Group(1), (), Orientation::Horizontal);
        let before = workspace.view_layout();
        workspace.resize_split(&[], vec![1.0]);
        assert_eq!(workspace.view_layout(), before);
    }
}
