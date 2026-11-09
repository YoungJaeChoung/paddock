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
use paddock_core::file_pane::{FilePane, FilePanes, FileTab, file_key, file_root_for, followed_key};
use paddock_core::layout_model::{Layout, Orientation};
use paddock_core::pane_columns::{attached_tab_ids, tab_columns};
use paddock_core::session_state::SessionState;
use paddock_core::tab_drop::{DropMode, is_same_place};
use paddock_core::work_model::{Candidate, GroupKey, NewPlace, Promotion, WorkState};

/// 다른 창의 파일 탭이 돌아올 묶음이 사라졌을 때의 묶음 이름표. 터미널 번호는 1부터라 실제 묶음과 겹치지 않는다.
const NO_HOME: u32 = 0;

/// 탭 줄 순서를 정할 때 쓰는 본문의 가상 크기. 칸 크기가 0이 아닌 한 열 순서는 크기와 무관하다.
const NOMINAL_BODY: f32 = 1000.0;

/// 탭 줄의 탭 하나: 터미널 또는 지금 보는 묶음 파일 칸의 N번째 파일 탭.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabRef {
    Terminal(u32),
    File(usize),
}

/// 다른 창으로 옮겨져 이 창의 칸 배치에서 빠진 탭. 탭·세션·Work 목록의 소속은 그대로이고 자리(어느 창의 어느 칸)만 다르다.
///
/// 파일 탭은 터미널과 달리 묶음의 파일 칸에 속해 있어 따로 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DetachedTab {
    Terminal(u32),
}

/// 다른 창으로 옮겨져 파일 칸에서 빠진 파일 탭. 열쇠(`FileTab::key`)로 가리키고, 파일 내용·저장하지 않은 표시는 창이 그대로 쥔다.
/// `group`은 되돌아올 묶음(그 묶음 파일 칸의 끝)이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedFile {
    pub group: GroupKey,
    pub tab: FileTab,
}

/// `Workspace::detach_terminal`이 분리하지 못한 까닭.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachError {
    /// 그런 탭이 없다.
    Unknown,
    /// 이미 다른 창에 있다.
    AlreadyDetached,
}

/// 되돌아오는 탭이 놓일 자리: 기준 탭이 든 칸에 `mode`로 놓는다(가장자리면 그 칸을 나누고, 탭 방식이면 그 칸의 끝에 붙인다).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockPlace {
    pub reference: u32,
    pub mode: DropMode,
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
    /// 다른 창에 가 있는 탭. 저장 상태에는 적지 않는다(다음 시작에는 모두 첫 칸 끝으로 돌아온다).
    detached: Vec<DetachedTab>,
    /// 다른 창에 가 있는 파일 탭(분리한 차례). 저장 상태에는 되돌아올 묶음의 파일 칸에 섞어 적는다.
    detached_files: Vec<DetachedFile>,
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
            detached: Vec::new(),
            detached_files: Vec::new(),
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
            self.detached.retain(|detached| *detached != DetachedTab::Terminal(tab.id));
        }
        // 쫓겨난 탭의 묶음 칸을 먼저 닫는다. 새 데몬이 같은 번호를 다시 매기므로, 그대로 두면 옛 묶음의 칸이 같은 번호의 새 묶음에 붙는다.
        self.follow_files(&work_before);
        let work_before = self.work.clone();
        match place {
            NewPlace::Folder(key) => self.work.assign_terminal(id, key, name),
            NewPlace::Group(root) => self.work.join_group(*root, id, name),
            NewPlace::NewGroup => self.work.add_group(id, name),
        }
        self.tabs.push(Tab { id, daemon: daemon.to_string(), content });
        self.selected = Some(id);
        self.follow_files(&work_before);
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

    /// 작업 목록이 `before`에서 지금으로 바뀐 뒤 파일 칸과 다른 창의 파일 탭이 돌아올 묶음을 새 이름표로 옮긴다.
    /// 묶음이 사라졌으면 다른 창의 파일 탭은 돌아올 곳 없음(`NO_HOME`)이 되어 돌아올 때 지금 보는 묶음에 붙는다.
    fn follow_files(
        &mut self,
        before: &WorkState,
    ) {
        self.files.follow_groups(before, &self.work);
        for detached in &mut self.detached_files {
            detached.group = followed_key(before, &self.work, &detached.group).unwrap_or(GroupKey::Group(NO_HOME));
        }
    }

    /// `Show to the Side`가 `id`를 옮길 때 기준이 될 터미널. 지금 보는 묶음의 터미널이고 기준이 될 다른 칸이 있을 때만 있다.
    pub fn beside_reference(
        &self,
        id: u32,
    ) -> Option<u32> {
        self.view_layout()
            .filter(|layout| layout.includes(id))
            .and_then(|layout| layout.beside_reference(id, self.selected))
    }

    /// `Show to the Side`: 터미널 `id`를 새로 만들지 않고 기준 터미널의 칸 오른쪽 새 칸으로 옮기고 선택한다.
    /// 탭(세션)은 그대로라 실행 중인 프로그램과 출력이 이어진다. 옮길 수 없으면 아무것도 바꾸지 않고 false다.
    pub fn show_beside(
        &mut self,
        id: u32,
    ) -> bool {
        let ids = self.group_ids(id);
        let moved = self
            .beside_reference(id)
            .and_then(|reference| self.layout_for(&ids).and_then(|layout| layout.moved_beside(id, reference)));
        let done = moved.is_some();
        if let Some(layout) = moved {
            self.commit_layout(&ids, layout);
            self.select(id);
        }
        done
    }

    /// 터미널 `id`를 다른 창으로 옮긴 것으로 표시하고 칸 배치에서 뺀다. 탭·세션·소속은 그대로다.
    ///
    /// 선택한 터미널이면 닫기와 같은 규칙으로 같은 묶음의 다른 탭에 선택을 넘긴다(넘길 탭이 없으면 선택은 그대로다).
    /// 전환 중인 탭의 거부처럼 창이 아는 사정은 호출자가 먼저 걸러 낸다.
    pub fn detach_terminal(
        &mut self,
        id: u32,
    ) -> Result<(), DetachError> {
        if self.tab(id).is_none() {
            Err(DetachError::Unknown)
        } else if self.is_detached(id) {
            Err(DetachError::AlreadyDetached)
        } else {
            let was_selected = self.selected == Some(id);
            let row = self.view_ids();
            self.detached.push(DetachedTab::Terminal(id));
            self.layouts = self.layouts.iter().filter_map(|layout| layout.without(&[id])).collect();
            if was_selected && let Some(next) = Self::neighbor_in_row(&row, id) {
                self.set_selected(next);
            }
            Ok(())
        }
    }

    /// 다른 창에 있던 터미널 `id`를 이 창의 배치로 되돌린다. 분리된 탭이 아니면 false다.
    ///
    /// `place`가 있으면 기준 탭의 칸에 그 방식으로 놓고, 없거나 기준을 찾을 수 없으면 묶음의 첫 칸 끝에 붙인다.
    /// 돌아온 탭은 자기 칸의 선택된 탭이 된다. 창의 선택(`selected`)은 그대로 두되, 지금 선택이 이 묶음의 다른 창 탭이면 돌아온 탭으로 옮긴다.
    pub fn reattach_terminal(
        &mut self,
        id: u32,
        place: Option<DockPlace>,
    ) -> bool {
        let mut done = false;
        if self.is_detached(id) {
            self.detached.retain(|tab| *tab != DetachedTab::Terminal(id));
            let ids = self.group_ids(id);
            let others: Vec<u32> = ids.iter().copied().filter(|other| *other != id).collect();
            let base = self.layout_for(&others);
            let placed = place.and_then(|place| match (&base, place.mode.side()) {
                (Some(base), Some(side)) => base.moved_split(id, place.reference, side),
                (Some(base), None) => base.moved_into(id, place.reference),
                (None, _) => None,
            });
            let arranged = placed.unwrap_or_else(|| Layout::with_terminal(base.as_ref(), id));
            self.commit_layout(&ids, arranged.select(id));
            if self.selected.is_some_and(|selected| ids.contains(&selected) && self.is_detached(selected)) {
                self.selected = Some(id);
            }
            done = true;
        }
        done
    }

    /// 이 창의 칸에 있는 터미널 `id`를 끌어 놓은 자리 `place`로 옮기고 선택한다. 바뀌는 것이 없으면(같은 칸 가운데, 이미 그 자리, 칸이
    /// 하나뿐인 탭을 그 칸 가장자리로, 기준이 이 묶음에 없음) 아무것도 바꾸지 않고 false다. 다른 창에 있는 탭은 `reattach_terminal`의 몫이다.
    ///
    /// 가장자리는 기준 칸을 그 방향으로 나눠 새 칸에 두고, 탭 방식은 기준 탭이 든 칸의 끝(`TabAfter`)·기준 탭 앞(`TabBefore`)에 둔다.
    /// 같은 칸이면 칸 안 순서만 바뀌고, 다른 칸이면 그 칸으로 옮긴 뒤 자리를 맞춘다.
    pub fn dock_terminal(
        &mut self,
        id: u32,
        place: DockPlace,
    ) -> bool {
        let mut done = false;
        if self.tab(id).is_some() && !self.is_detached(id) {
            let ids = self.group_ids(id);
            let docked = self.layout_for(&ids).and_then(|layout| Self::docked(&layout, id, place));
            if let Some(layout) = docked {
                self.commit_layout(&ids, layout.select(id));
                self.select(id);
                done = true;
            }
        }
        done
    }

    /// `layout`에서 `id`를 `place`로 옮긴 배치. 바뀌는 것이 없으면 None이다.
    fn docked(
        layout: &Layout,
        id: u32,
        place: DockPlace,
    ) -> Option<Layout> {
        let DockPlace { reference, mode } = place;
        match mode.side() {
            Some(side) => layout.moved_split(id, reference, side),
            None => {
                let boxes = layout.pane_boxes(1.0, 1.0);
                let own = boxes.iter().find(|pane| pane.tabs.contains(&id));
                if id == reference || own.is_some_and(|pane| is_same_place(&pane.tabs, id, mode, reference)) {
                    None
                } else {
                    let before = mode == DropMode::TabBefore;
                    layout
                        .moved_within(id, reference, before)
                        .or_else(|| layout.moved_into(id, reference)?.moved_within(id, reference, before))
                }
            }
        }
    }

    /// 터미널이 다른 창에 있는지.
    pub fn is_detached(
        &self,
        id: u32,
    ) -> bool {
        self.detached.contains(&DetachedTab::Terminal(id))
    }

    /// 분리 창이 열려 있는 터미널 번호들 `open` 중, 더는 다른 창에 있지 않은 것(탭이 닫혔거나 데몬이 바뀌어 쫓겨났다). 창이 그 분리 창을 정리하는 데 쓴다.
    pub fn stale_detached_ids(
        &self,
        open: &[u32],
    ) -> Vec<u32> {
        open.iter().copied().filter(|id| !self.is_detached(*id)).collect()
    }

    /// 다른 창에 있는 탭들(분리한 차례).
    pub fn detached(&self) -> &[DetachedTab] {
        &self.detached
    }

    /// 다른 창에 있는 터미널의 번호(분리한 차례).
    pub fn detached_terminals(&self) -> Vec<u32> {
        self.detached
            .iter()
            .map(|tab| match tab {
                DetachedTab::Terminal(id) => *id,
            })
            .collect()
    }

    /// 지금 보는 묶음의 터미널 중 다른 창에 있는 것(묶음의 탭 목록 순서). 탭 줄이 흐린 탭으로 남길 때 쓴다.
    pub fn view_detached_ids(&self) -> Vec<u32> {
        let group = self.selected.map(|selected| self.group_ids(selected)).unwrap_or_default();
        group.into_iter().filter(|id| self.is_detached(*id)).collect()
    }

    /// 탭 줄 `row`에서 `id`가 빠졌을 때 그 자리를 이을 탭: 같은 자리에 들어온 다음 탭, 없으면 이전 탭이다.
    fn neighbor_in_row(
        row: &[u32],
        id: u32,
    ) -> Option<u32> {
        let position = row.iter().position(|other| *other == id).unwrap_or(0);
        let remaining: Vec<u32> = row.iter().copied().filter(|other| *other != id).collect();
        remaining.get(position).or_else(|| position.checked_sub(1).and_then(|before| remaining.get(before))).copied()
    }

    /// 탭 줄이 열마다 그릴 탭: (터미널 번호, 다른 창에 있는지). 다른 창에 있는 탭은 칸이 없어 마지막 열 끝에 붙는다.
    ///
    /// 칸을 나눈 줄(열이 둘 이상)에서는 다른 창의 탭을 뺀다 — 그대로 두면 다른 칸 위에 떠서 분리한 탭이 옛 칸에 남은 것처럼 보인다.
    /// 한 열이면 흐린 탭으로 줄 끝에 남겨 되돌릴 입구를 둔다. 보일 칸이 하나도 없으면(묶음의 탭이 모두 다른 창에 있음) 열 하나에 흐린 탭만 든다.
    pub fn strip_columns(
        &self,
        width: f32,
        height: f32,
    ) -> Vec<Vec<(u32, bool)>> {
        let mut columns: Vec<Vec<u32>> = match self.view_layout() {
            Some(layout) => tab_columns(&layout, width, height).into_iter().map(|column| column.tabs).collect(),
            None => vec![Vec::new()],
        };
        // 파일 칸은 터미널 칸 옆의 또 하나의 열이다. 파일을 연 줄도 칸을 나눈 줄처럼 다른 창의 탭을 뺀다.
        let is_split = columns.len() > 1 || self.file_pane().is_some();
        let all: Vec<u32> = columns.iter().flatten().copied().chain(self.view_detached_ids()).collect();
        let trailing: Vec<u32> = attached_tab_ids(&all, is_split, |id| self.is_detached(id))
            .into_iter()
            .filter(|id| self.is_detached(*id))
            .collect();
        if let Some(last) = columns.last_mut() {
            last.extend(trailing);
        }
        columns.into_iter().map(|column| column.into_iter().map(|id| (id, self.is_detached(id))).collect()).collect()
    }

    /// 선택을 바꾼다. 없는 id면 아무것도 바꾸지 않고 false다. 묶음에서 마지막으로 쓴 터미널로 기록한다.
    ///
    /// 다른 창에 있는 터미널이면 그 묶음에서 이 창에 있는 탭이 선택으로 남는다(지금 선택이 그 묶음 것이면 그대로, 아니면 첫 칸의 첫 탭).
    /// 묶음의 탭이 모두 다른 창에 있을 때만 그 터미널이 선택이 된다(본문은 비고 탭 줄은 흐린 탭을 보인다).
    pub fn select(
        &mut self,
        id: u32,
    ) -> bool {
        let found = self.tab(id).is_some();
        if found {
            let chosen = if self.is_detached(id) { self.attached_stand_in(id).unwrap_or(id) } else { id };
            self.set_selected(chosen);
            self.work.touch(chosen);
        }
        found
    }

    /// 다른 창에 있는 터미널 `id`를 대신해 선택할 같은 묶음의 이 창 탭. 지금 선택이 그 묶음의 이 창 탭이면 그것, 없으면 첫 칸의 첫 탭이다.
    fn attached_stand_in(
        &self,
        id: u32,
    ) -> Option<u32> {
        let ids = self.group_ids(id);
        let layout = self.layout_for(&ids);
        let current = self.selected.filter(|selected| ids.contains(selected) && !self.is_detached(*selected));
        current.or_else(|| layout.and_then(|layout| layout.terminal_ids().first().copied()))
    }

    /// 선택을 정한다. 저장된 선택을 되살릴 때 쓰며, 없는 id거나 None이면 첫 탭을 고른다.
    pub fn select_or_first(
        &mut self,
        id: Option<u32>,
    ) {
        let chosen = id.filter(|id| self.tab(*id).is_some()).or_else(|| self.tabs.first().map(|tab| tab.id));
        // 다른 창에 있는 탭은 선택이 될 수 없다(묶음에 이 창 탭이 있을 때): `select`와 같은 규칙으로 이 창 탭을 고른다.
        let chosen = chosen.map(|chosen| {
            if self.is_detached(chosen) { self.attached_stand_in(chosen).unwrap_or(chosen) } else { chosen }
        });
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
    /// 다른 창에 있는 터미널(`detached`)은 이 창의 배치에 들지 않는다. 보일 터미널이 없으면 None이다.
    fn layout_for(
        &self,
        ids: &[u32],
    ) -> Option<Layout> {
        let shown: Vec<u32> = ids.iter().copied().filter(|id| !self.is_detached(*id)).collect();
        let open: HashSet<u32> = shown.iter().copied().collect();
        let mut layout = Layout::besides(self.layouts.iter().map(|saved| saved.without_closed(&open)).collect());
        for id in &shown {
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
        self.detached.retain(|tab| *tab != DetachedTab::Terminal(id));
        let work_before = self.work.clone();
        self.work.forget_terminal(id);
        self.follow_files(&work_before);
        if was_selected {
            let next = Self::neighbor_in_row(&row, id).or(fallback).filter(|next| self.tab(*next).is_some());
            match next {
                Some(next) => self.set_selected(next),
                None => self.selected = None,
            }
        }
        Some(closed)
    }

    /// 터미널 `old`를 새 터미널 `new`로 바꾼다. 탭 자리·칸 배치·Work 목록의 행(폴더 안 위치 또는 Unassigned 묶음)·이름·선택·묶음 파일 칸이 그대로 새 번호로 넘어온다.
    ///
    /// 계정을 바꿔 같은 탭에서 새 CLI를 띄울 때 쓴다(데몬의 터미널 번호는 프로세스마다 새로 매겨져, 탭을 번호째 옮긴다).
    /// 옛 탭의 내용을 돌려준다(호출자가 그 터미널을 끝낸다). `old`가 없으면 아무것도 바꾸지 않고 None이다.
    pub fn replace_terminal(
        &mut self,
        old: u32,
        new: u32,
        daemon: &str,
        content: T,
    ) -> Option<Tab<T>> {
        let index = self.tabs.iter().position(|tab| tab.id == old)?;
        let mut new_ids: std::collections::BTreeMap<u32, u32> = self.tabs.iter().map(|tab| (tab.id, tab.id)).collect();
        new_ids.insert(old, new);
        let state = self.snapshot().renumbered(&new_ids, daemon);
        self.work = state.work;
        self.layouts = state.layouts;
        self.files = state.files;
        self.selected = state.selected;
        for tab in &mut self.detached {
            match tab {
                DetachedTab::Terminal(id) if *id == old => *id = new,
                DetachedTab::Terminal(_) => {}
            }
        }
        for detached in &mut self.detached_files {
            if let GroupKey::Group(root) = detached.group {
                // 묶음의 시작 터미널이 바뀌면 묶음 이름표도 새 번호로 따라간다(`renumbered`와 같다).
                if let Some(new_root) = new_ids.get(&root) {
                    detached.group = GroupKey::Group(*new_root);
                }
            }
        }
        Some(std::mem::replace(&mut self.tabs[index], Tab { id: new, daemon: daemon.to_string(), content }))
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
        self.follow_files(&work_before);
        promotion
    }

    /// 작업 폴더를 목록에서 뺀다(그 폴더의 터미널 소속도 함께 지운다). 그 폴더의 파일 칸도 사라진다. 디스크의 폴더는 건드리지 않는다.
    pub fn remove_folder(
        &mut self,
        key: &str,
    ) {
        let work_before = self.work.clone();
        self.work.remove_folder(key, None);
        self.follow_files(&work_before);
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

    /// 파일·폴더의 이름이 `from`에서 `to`로 바뀐 뒤 모든 묶음의 파일 탭을 새 경로로 옮긴다. 옮긴 탭이 있으면 true다.
    pub fn rename_file_paths(
        &mut self,
        from: &str,
        to: &str,
    ) -> bool {
        let mut moved = self.files.rename_path(from, to);
        // 다른 창에 있는 탭도 새 경로를 따라간다.
        for detached in &mut self.detached_files {
            if let Some(path) = paddock_core::paths::rebased(from, to, &detached.tab.path) {
                detached.tab.path = file_key(&path);
                moved = true;
            }
        }
        moved
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
        // 같은 파일의 다른 철자는 같은 탭이다(호출자가 이미 맞춘 경로도 그대로 통과한다).
        let path = &file_key(path);
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

    /// 묶음 `group`의 파일 칸에서 `index`번째 탭을 다른 창으로 옮긴 것으로 표시하고 칸에서 뺀다. 칸의 선택·비우기는 탭을 닫을 때와 같다.
    /// 파일 내용·저장하지 않은 표시는 창이 그대로 쥔다. 그런 탭이 없으면 None이다.
    pub fn detach_file(
        &mut self,
        group: &GroupKey,
        index: usize,
    ) -> Option<DetachedFile> {
        let tab = self.files.take(group, index)?;
        let detached = DetachedFile { group: group.clone(), tab };
        self.detached_files.push(detached.clone());
        Some(detached)
    }

    /// 다른 창에 있던 파일 탭(열쇠 `key`)을 되돌아올 묶음의 파일 칸 끝에 붙이고 선택한다. 그 묶음이 사라졌으면 지금 보는 묶음(없으면 맨 위
    /// 터미널의 묶음)에 붙는다. 붙은 묶음 이름표와 탭 번호를 돌려준다. 그런 탭이 없거나 붙일 묶음이 없으면 아무것도 바꾸지 않고 None이다.
    pub fn reattach_file(
        &mut self,
        key: &str,
    ) -> Option<(GroupKey, usize)> {
        let at = self.detached_files.iter().position(|detached| detached.tab.key() == key)?;
        let home = &self.detached_files[at].group;
        let exists = match home {
            GroupKey::Folder(folder) => self.work.folders.iter().any(|candidate| candidate.key == *folder),
            GroupKey::Group(root) => !self.work.group_members(*root).is_empty(),
        };
        let top = self.tabs.first().and_then(|tab| self.work.group_key_of(tab.id));
        let target = if exists { Some(home.clone()) } else { self.view_key().or(top) }?;
        let detached = self.detached_files.remove(at);
        let index = self.files.open(&target, &detached.tab.path, detached.tab.kind);
        Some((target, index))
    }

    /// 다른 창에 있는 파일 탭(열쇠 `key`)을 돌아올 곳 없이 지운다(탭을 닫았다). 그런 탭이 없으면 false다.
    pub fn close_detached_file(
        &mut self,
        key: &str,
    ) -> bool {
        let before = self.detached_files.len();
        self.detached_files.retain(|detached| detached.tab.key() != key);
        self.detached_files.len() != before
    }

    /// 다른 창에 있는 파일 탭(열쇠 `key`)이 보여 주는 방식을 바꾼다(이진 안내 탭 → 텍스트). 열쇠는 그대로다.
    pub fn set_detached_file_kind(
        &mut self,
        key: &str,
        kind: FileKind,
    ) {
        if let Some(detached) = self.detached_files.iter_mut().find(|detached| detached.tab.key() == key) {
            detached.tab.kind = kind;
        }
    }

    /// 다른 창에 있는 파일 탭들(분리한 차례).
    pub fn detached_files(&self) -> &[DetachedFile] {
        &self.detached_files
    }

    /// 열쇠 `key`의 파일 탭이 다른 창에 있으면 그 탭.
    pub fn detached_file(
        &self,
        key: &str,
    ) -> Option<&DetachedFile> {
        self.detached_files.iter().find(|detached| detached.tab.key() == key)
    }

    /// 열쇠 `key`의 파일 탭이 다른 창에 있는지.
    pub fn is_file_detached(
        &self,
        key: &str,
    ) -> bool {
        self.detached_file(key).is_some()
    }

    /// 열쇠 `key`의 파일 탭이 어느 묶음 파일 칸의 몇 번째에 열려 있는지(다른 창에 있는 탭은 찾지 않는다).
    pub fn find_file_tab(
        &self,
        key: &str,
    ) -> Option<(GroupKey, usize)> {
        self.files.iter().find_map(|(group, pane)| {
            pane.tabs.iter().position(|tab| tab.key() == key).map(|index| (group.clone(), index))
        })
    }

    /// 어느 묶음에서든(다른 창 포함) 열려 있는 파일 탭들.
    pub fn file_tabs(&self) -> impl Iterator<Item = &FileTab> {
        self.files
            .iter()
            .flat_map(|(_, pane)| pane.tabs.iter())
            .chain(self.detached_files.iter().map(|detached| &detached.tab))
    }

    /// 어느 묶음에서든(다른 창 포함) 열려 있는 탭의 열쇠.
    pub fn open_file_keys(&self) -> impl Iterator<Item = String> {
        self.file_tabs().map(FileTab::key)
    }

    /// 지금 탭 줄이 보여 주는 탭의 순서: 터미널 탭 줄 순서(`view_ids`) 뒤에 파일 칸의 탭이 이어진다. 탭 이동 키가 이 순서로 돈다.
    pub fn tab_order(&self) -> Vec<TabRef> {
        let n_files = self.file_pane().map_or(0, |pane| pane.tabs.len());
        self.view_ids().into_iter().map(TabRef::Terminal).chain((0..n_files).map(TabRef::File)).collect()
    }

    /// state.json에 저장할 상태. 배치는 탭이 있는 터미널만 남긴다. 파일 목록 구획의 접힘(`files_collapsed`)과 상태 줄 사용량 선택(`usage_selection`)은 창이 쥐므로 호출자가 채운다.
    ///
    /// 다른 창에 있는 파일 탭은 돌아올 묶음의 파일 칸 끝에 섞어 적는다(다음 시작에는 분리 창이 없으므로 그 자리로 돌아온다).
    pub fn to_state(&self) -> SessionState {
        let mut state = self.snapshot();
        for detached in &self.detached_files {
            // 돌아올 묶음이 사라진 탭은 지금 보는 묶음에 섞는다(돌아올 때와 같다).
            let home = match detached.group {
                GroupKey::Group(NO_HOME) => {
                    let top = self.tabs.first().and_then(|tab| self.work.group_key_of(tab.id));
                    match self.view_key().or(top) {
                        Some(home) => home,
                        // 붙일 묶음이 하나도 없으면(터미널이 없다) 복원할 곳이 없다.
                        None => continue,
                    }
                }
                _ => detached.group.clone(),
            };
            let mut pane = state.files.get(&home).cloned().unwrap_or(FilePane {
                tabs: Vec::new(),
                selected: 0,
                ratio: paddock_core::file_pane::DEFAULT_RATIO,
            });
            if !pane.tabs.iter().any(|tab| tab.key() == detached.tab.key()) {
                pane.tabs.push(detached.tab.clone());
            }
            state.files.insert(&home, pane);
        }
        state
    }

    /// 다른 창의 파일 탭을 섞지 않은 상태(번호를 새로 매길 때 쓴다).
    fn snapshot(&self) -> SessionState {
        let open: HashSet<u32> = self.tabs.iter().map(|tab| tab.id).collect();
        SessionState {
            daemon: self.tabs.first().map(|tab| tab.daemon.clone()).unwrap_or_default(),
            work: self.work.clone(),
            selected: self.selected,
            layouts: self.layouts.iter().filter_map(|layout| layout.without_closed(&open)).collect(),
            sidebar_collapsed: self.sidebar_collapsed,
            files: self.files.clone(),
            files_collapsed: false,
            usage_selection: Default::default(),
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

    #[test]
    fn test_c_d78_replace_tab_terminal_keeps_the_tab_slot_pane_and_work_row() {
        // 폴더 안 터미널 둘(1·2)과 Unassigned 묶음 [3, 4]. 가운데 탭과 묶음의 둘째를 바꿔도 자리·이름·선택이 그대로다.
        let mut workspace: Workspace<&str> = Workspace::default();
        workspace.add(DAEMON, 1, "one", &NewPlace::Folder("/w".into()), "a");
        workspace.add(DAEMON, 2, "two", &NewPlace::Folder("/w".into()), "b");
        workspace.add(DAEMON, 3, "three", &NewPlace::NewGroup, "c");
        workspace.add(DAEMON, 4, "four", &NewPlace::Group(3), "d");
        workspace.select(2);
        let before_layout = workspace.view_layout();
        let old = workspace.replace_terminal(2, 20, DAEMON, "b2").unwrap();
        assert_eq!((old.id, old.content), (2, "b"));
        let tabs: Vec<(u32, &str)> = workspace.tabs().iter().map(|tab| (tab.id, tab.content)).collect();
        assert_eq!(tabs, [(1, "a"), (20, "b2"), (3, "c"), (4, "d")], "the tab keeps its slot");
        assert_eq!(workspace.selected(), Some(20), "the selection follows the replaced terminal");
        assert_eq!(workspace.name(20), "two");
        assert_eq!(workspace.work().folder_of(20), Some("/w"));
        assert_eq!(workspace.work().terminals_of("/w"), [1, 20]);
        assert!(workspace.work().name_of(2).is_none(), "the old number is gone from the list");
        assert_eq!(
            workspace.view_layout().map(|layout| layout.terminal_ids()),
            before_layout.map(|layout| layout
                .terminal_ids()
                .iter()
                .map(|id| if *id == 2 { 20 } else { *id })
                .collect())
        );
        // 묶음의 시작 터미널을 바꾸면 묶음 이름표도 새 번호로 따라간다.
        workspace.select(3);
        workspace.replace_terminal(3, 30, DAEMON, "c2").unwrap();
        assert_eq!(workspace.work().group_members(30), [30, 4]);
        assert!(workspace.replace_terminal(99, 100, DAEMON, "x").is_none());
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
    fn test_one_file_spelled_three_ways_is_one_tab() {
        let mut workspace = two_groups();
        let first = workspace.open_file("/tmp/a.txt", FileKind::Text);
        assert_eq!(workspace.open_file("/tmp/./x/../a.txt", FileKind::Text), first);
        assert_eq!(workspace.open_file("/tmp/./a.txt", FileKind::Text), first);
        assert_eq!(workspace.file_pane().unwrap().tabs.len(), 1);
        // 드라이브 경로의 구분자·대소문자 철자.
        let drive = workspace.open_file("C:\\x\\a.js", FileKind::Text);
        assert_eq!(workspace.open_file("C:/x/a.js", FileKind::Text), drive);
        assert_eq!(workspace.open_file("c:/x/./a.js", FileKind::Text), drive);
        let paths: Vec<String> = workspace.file_pane().unwrap().tabs.iter().map(|tab| tab.path.clone()).collect();
        assert_eq!(paths, ["/tmp/a.txt", "C:/x/a.js"]);
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

    #[test]
    fn test_c_d71_show_beside_keeps_the_terminal_alive() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.select(1);
        let tabs_before: Vec<u32> = workspace.tabs().iter().map(|tab| tab.id).collect();
        assert!(workspace.show_beside(3));
        // 터미널(세션)은 하나도 새로 만들어지거나 사라지지 않고, 3이 1의 오른쪽 새 칸에서 선택된다.
        let tabs_after: Vec<u32> = workspace.tabs().iter().map(|tab| tab.id).collect();
        assert_eq!(tabs_after, tabs_before);
        assert_eq!(workspace.selected(), Some(3));
        assert_eq!(workspace.view_layout(), Some(two_panes(Orientation::Horizontal, pane(&[1, 2], 0), pane(&[3], 0))));
    }

    #[test]
    fn test_c_d71_show_beside_needs_a_reference_pane_and_a_terminal_of_the_shown_group() {
        // 터미널이 하나뿐이면 기준이 없어 아무것도 하지 않는다.
        let mut alone = workspace(&[1]);
        assert!(!alone.show_beside(1));
        assert_eq!(alone.view_layout(), Some(pane(&[1], 0)));
        // 다른 묶음의 터미널은 옮기지 않는다.
        let mut groups = two_groups();
        let layout_before = groups.view_layout();
        let other = groups.tabs().iter().map(|tab| tab.id).find(|id| groups.beside_reference(*id).is_none()).unwrap();
        assert!(!groups.show_beside(other));
        assert_eq!(groups.view_layout(), layout_before);
        // 한 칸에 보이는 탭이 옮길 터미널 하나뿐이면 기준이 없다. 같은 칸의 숨은 탭을 옮길 때는 보이는 탭이 기준이다.
        let mut tabs = workspace(&[1, 2]);
        tabs.select(2);
        assert_eq!(tabs.beside_reference(2), None);
        assert!(!tabs.show_beside(2));
        tabs.select(1);
        assert_eq!(tabs.beside_reference(2), Some(1));
        assert!(tabs.show_beside(2));
        assert_eq!(tabs.view_layout(), Some(two_panes(Orientation::Horizontal, pane(&[1], 0), pane(&[2], 0))));
    }

    #[test]
    fn test_c_d71_drag_reorders_only_inside_the_same_folder() {
        let mut workspace = Workspace::<()>::default();
        for (id, folder) in [(1, "/a"), (2, "/a"), (3, "/a"), (4, "/b")] {
            workspace.add(DAEMON, id, &format!("terminal {id}"), &NewPlace::Folder(folder.to_string()), ());
        }
        let order = |workspace: &Workspace<()>| workspace.work().terminals_of("/a");
        assert_eq!(order(&workspace), vec![1, 2, 3]);
        // 같은 폴더의 행끼리는 놓은 자리(앞·뒤)대로 순서가 바뀐다.
        assert!(workspace.work().can_reorder(3, 1));
        workspace.work_mut().move_terminal(3, 1, false);
        assert_eq!(order(&workspace), vec![3, 1, 2]);
        workspace.work_mut().move_terminal(3, 2, true);
        assert_eq!(order(&workspace), vec![1, 2, 3]);
        // 다른 폴더의 행에는 놓을 수 없고, 놓아도 아무것도 바뀌지 않는다.
        assert!(!workspace.work().can_reorder(1, 4));
        workspace.work_mut().move_terminal(1, 4, true);
        assert_eq!(order(&workspace), vec![1, 2, 3]);
        assert_eq!(workspace.work().terminals_of("/b"), vec![4]);
    }

    #[test]
    fn test_c_d82_a_detached_terminal_leaves_the_layout_but_stays_in_tabs_and_work() {
        let mut workspace = workspace(&[1, 2, 3]);
        assert_eq!(workspace.detach_terminal(2), Ok(()));
        assert_eq!(ids(&workspace), vec![1, 2, 3], "the tab is still there");
        assert_eq!(workspace.view_ids(), vec![1, 3]);
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 3]));
        assert_eq!(workspace.name(2), "terminal 2", "the Work list keeps the row and the name");
        assert_eq!(workspace.work().group_members(1), [1, 2, 3]);
        assert!(workspace.is_detached(2) && !workspace.is_detached(1));
        assert_eq!(workspace.detached_terminals(), vec![2]);
        assert_eq!(workspace.view_detached_ids(), vec![2]);
        assert_eq!(workspace.selected(), Some(3), "a tab that stays selected is untouched");
        assert_eq!(workspace.detach_terminal(2), Err(DetachError::AlreadyDetached));
        assert_eq!(workspace.detach_terminal(99), Err(DetachError::Unknown));
    }

    #[test]
    fn test_c_d81_move_to_new_window_keeps_the_session_pid_and_grid() {
        // 탭의 내용(세션·셸 pid·격자)은 창을 옮겨도 같은 값이다: 옮기는 것은 자리뿐이다.
        let mut workspace: Workspace<(u32, &str)> = Workspace::default();
        workspace.add(DAEMON, 1, "terminal 1", &NewPlace::NewGroup, (4242, "grid-1"));
        workspace.add(DAEMON, 2, "terminal 2", &NewPlace::Group(1), (4343, "grid-2"));
        workspace.detach_terminal(2).unwrap();
        assert_eq!(workspace.tab(2).map(|tab| tab.content), Some((4343, "grid-2")));
        workspace.reattach_terminal(2, None);
        assert_eq!(workspace.tab(2).map(|tab| tab.content), Some((4343, "grid-2")));
        assert_eq!(workspace.tab(1).map(|tab| tab.content), Some((4242, "grid-1")));
    }

    #[test]
    fn test_c_d84_closing_a_detached_window_moves_the_tab_back_to_the_end_of_the_first_pane() {
        let mut workspace = workspace(&[1, 2]);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        assert_eq!(workspace.view_layout().map(|layout| layout.pane_boxes(1000.0, 600.0).len()), Some(2));
        // 둘째 칸의 유일한 탭 3을 분리하면 칸이 하나로 합쳐지고, 창을 닫으면 3은 옛 둘째 칸이 아니라 첫 칸의 끝으로 돌아온다.
        workspace.detach_terminal(3).unwrap();
        assert_eq!(workspace.view_layout().map(|layout| layout.pane_boxes(1000.0, 600.0).len()), Some(1));
        assert!(workspace.reattach_terminal(3, None));
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 2, 3]));
        assert_eq!(workspace.view_layout().map(|layout| layout.pane_boxes(1000.0, 600.0).len()), Some(1));
        assert_eq!(workspace.selected(), Some(2), "the model keeps the selection; the window selects the returned tab");
    }

    #[test]
    fn test_c_d84_a_terminal_that_ends_while_detached_loses_its_mark_so_its_window_closes() {
        // 분리 창은 `is_detached`가 거짓이 되면 스스로 닫힌다. 탭이 닫히면(터미널 종료 뒤 닫기·× 확인) 표지가 함께 사라져야 한다.
        let mut workspace = workspace(&[1, 2]);
        workspace.detach_terminal(2).unwrap();
        assert!(workspace.close(2).is_some());
        assert!(!workspace.is_detached(2));
        assert!(workspace.detached_terminals().is_empty());
        assert_eq!(workspace.view_ids(), vec![1]);
    }

    #[test]
    fn test_c_d85_a_file_pane_counts_as_a_column_so_a_detached_tab_leaves_the_strip() {
        // 파일 칸은 터미널 칸 옆의 또 하나의 열이다(Electron은 편집기 칸도 칸 위치에 든다). 파일을 연 줄에서는 분리한 탭이 흐린 탭으로 남지 않고 빠진다.
        let mut workspace = workspace(&[1, 2]);
        workspace.open_file("/tmp/a.txt", paddock_core::file_kind::FileKind::Text);
        workspace.detach_terminal(2).unwrap();
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(1, false)]]);
    }

    #[test]
    fn test_c_d82_adding_a_terminal_for_a_new_daemon_returns_evicted_detached_ones_for_the_window_to_close() {
        let mut workspace = workspace(&[1, 2]);
        workspace.detach_terminal(2).unwrap();
        let evicted = workspace.add("20@t1", 1, "terminal 1", &NewPlace::NewGroup, ());
        let mut ids: Vec<u32> = evicted.iter().map(|tab| tab.id).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2], "the detached tab is evicted too, so the window must close its detached window");
        assert!(workspace.detached().is_empty());
    }

    #[test]
    fn test_c_d82_a_window_open_for_an_evicted_number_is_stale_even_when_the_number_is_reused() {
        let mut workspace = workspace(&[1, 2]);
        workspace.detach_terminal(2).unwrap();
        assert_eq!(workspace.stale_detached_ids(&[2]), Vec::<u32>::new());
        // 데몬이 바뀌어 번호 2가 새 터미널로 다시 쓰여도 그 번호는 분리 상태가 아니다.
        workspace.add("20@t1", 2, "terminal 1", &NewPlace::NewGroup, ());
        assert_eq!(workspace.stale_detached_ids(&[2]), vec![2]);
    }

    #[test]
    fn test_c_d82_reattach_without_a_place_goes_to_the_end_of_the_first_pane() {
        let mut workspace = workspace(&[1, 2]);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 2, 3]));
        workspace.detach_terminal(1).unwrap();
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![2, 3]));
        assert!(workspace.reattach_terminal(1, None));
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![2, 1, 3]));
        assert!(!workspace.is_detached(1));
        assert!(!workspace.reattach_terminal(1, None), "a terminal that is not detached has nothing to reattach");
    }

    #[test]
    fn test_c_d82_reattach_with_an_edge_splits_the_reference_pane() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.detach_terminal(3).unwrap();
        let place = DockPlace { reference: 1, mode: DropMode::SplitRight };
        assert!(workspace.reattach_terminal(3, Some(place)));
        assert_eq!(
            workspace.view_layout(),
            Some(Layout::Split {
                orientation: Orientation::Horizontal,
                children: vec![Layout::pane(vec![1, 2], 1), Layout::pane(vec![3], 0)],
                sizes: vec![0.5, 0.5],
            })
        );
        // 탭 방식은 기준 탭이 든 칸의 끝에 붙고 그 칸의 선택된 탭이 된다.
        workspace.detach_terminal(2).unwrap();
        assert!(workspace.reattach_terminal(2, Some(DockPlace { reference: 3, mode: DropMode::TabAfter })));
        assert_eq!(
            workspace.view_layout().map(|layout| layout.terminal_ids()),
            Some(vec![1, 3, 2]),
            "2 joins the pane of 3"
        );
        // 기준을 찾을 수 없으면 첫 칸 끝이다.
        workspace.detach_terminal(1).unwrap();
        assert!(workspace.reattach_terminal(1, Some(DockPlace { reference: 77, mode: DropMode::SplitLeft })));
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![3, 2, 1]));
    }

    #[test]
    fn test_c_d83_docking_an_attached_terminal_splits_moves_or_reorders_and_selects_it() {
        let mut single = workspace(&[1]);
        let mut across = workspace(&[1, 2, 3]);
        let mut workspace = workspace(&[1, 2, 3]);
        let dock = |reference, mode| DockPlace { reference, mode };
        // 한 칸의 탭 둘을 가장자리로 끌면 칸이 나뉘고 끈 탭이 새 칸에서 선택된다(비율 0.5).
        assert!(workspace.dock_terminal(2, dock(1, DropMode::SplitRight)));
        assert_eq!(workspace.selected(), Some(2));
        assert_eq!(
            workspace.view_layout(),
            Some(Layout::Split {
                orientation: Orientation::Horizontal,
                children: vec![Layout::pane(vec![1, 3], 1), Layout::pane(vec![2], 0)],
                sizes: vec![0.5, 0.5],
            })
        );
        // 다른 칸 가운데는 그 칸의 끝에 붙는다.
        assert!(workspace.dock_terminal(3, dock(2, DropMode::TabAfter)));
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 2, 3]));
        // 칸 안에서 앞으로 끼우면 순서가 바뀌고, 같은 자리에 다시 끼우면 아무것도 바뀌지 않는다.
        assert!(workspace.dock_terminal(3, dock(2, DropMode::TabBefore)));
        assert_eq!(workspace.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 3, 2]));
        assert!(!workspace.dock_terminal(3, dock(2, DropMode::TabBefore)), "already before 2");
        // 자기 자신 기준은 바뀌는 것이 없다.
        assert!(!workspace.dock_terminal(2, dock(2, DropMode::TabAfter)));
        // 다른 칸 탭 앞에 끼우면 그 칸으로 옮겨 그 탭 앞에 놓인다.
        assert!(workspace.dock_terminal(1, dock(3, DropMode::TabBefore)));
        assert_eq!(workspace.view_layout(), Some(Layout::pane(vec![1, 3, 2], 0)));
        // 다른 칸 탭의 오른쪽 절반(기준 탭 뒤)에 놓으면 칸 끝이 아니라 그 탭 바로 뒤에 놓인다.
        assert!(across.dock_terminal(1, dock(3, DropMode::SplitLeft)));
        assert_eq!(across.view_layout().map(|layout| layout.terminal_ids()), Some(vec![1, 2, 3]));
        assert!(across.dock_terminal(1, dock(2, DropMode::TabAfter)));
        assert_eq!(across.view_layout().map(|layout| layout.terminal_ids()), Some(vec![2, 1, 3]));
        // 칸이 하나뿐인 탭을 제 칸 가장자리로 끌어도 나눌 곳이 없다. 다른 창에 있는 탭은 이 길이 아니다.
        assert!(!single.dock_terminal(1, dock(1, DropMode::SplitLeft)));
        workspace.detach_terminal(2).unwrap();
        assert!(!workspace.dock_terminal(2, dock(1, DropMode::SplitLeft)));
    }

    #[test]
    fn test_c_d82_selecting_a_detached_terminal_falls_back_like_close() {
        let mut workspace = workspace(&[1, 2, 3]);
        assert_eq!(workspace.selected(), Some(3));
        workspace.detach_terminal(3).unwrap();
        assert_eq!(workspace.selected(), Some(2), "the selected tab went to its left neighbour like a close");
        workspace.select(3);
        assert_eq!(
            workspace.selected(),
            Some(2),
            "a detached tab cannot be the selected tab of a group that has others"
        );
        // 묶음의 탭이 모두 다른 창에 있으면 선택은 그대로 두고 본문은 비어 있다.
        workspace.detach_terminal(2).unwrap();
        workspace.detach_terminal(1).unwrap();
        assert_eq!(workspace.selected(), Some(1));
        assert_eq!(workspace.view_layout(), None);
        assert_eq!(workspace.view_ids(), Vec::<u32>::new());
        assert_eq!(workspace.view_detached_ids(), vec![1, 2, 3]);
    }

    #[test]
    fn test_c_d82_replace_terminal_carries_the_detached_mark() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.detach_terminal(2).unwrap();
        workspace.replace_terminal(2, 20, DAEMON, ()).unwrap();
        assert!(workspace.is_detached(20));
        assert!(!workspace.is_detached(2));
        assert_eq!(workspace.view_ids(), vec![1, 3]);
        assert_eq!(workspace.detached(), [DetachedTab::Terminal(20)]);
    }

    #[test]
    fn test_c_d82_state_json_does_not_record_detachment() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.detach_terminal(2).unwrap();
        let state = workspace.to_state();
        assert!(state.layouts.iter().all(|layout| !layout.includes(2)), "the saved layout has no trace of 2");
        assert_eq!(state.terminal_ids(), vec![1, 2, 3], "the Work list still has 2");
        // 다음 시작에는 배치에 없는 터미널처럼 첫 칸 끝으로 돌아온다.
        let mut restored = Workspace::<()>::default();
        restored.set_work(state.work.clone());
        for id in [1, 2, 3] {
            restored.attach(DAEMON, id, ());
        }
        restored.set_layouts(state.layouts.clone());
        restored.select_or_first(state.selected);
        assert_eq!(restored.view_ids(), vec![1, 3, 2]);
        assert!(restored.detached().is_empty());
    }

    #[test]
    fn test_c_d82_closing_a_detached_terminal_or_losing_its_daemon_forgets_the_mark() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.detach_terminal(2).unwrap();
        workspace.close(2).unwrap();
        assert!(workspace.detached().is_empty());
        workspace.detach_terminal(1).unwrap();
        workspace.add("20@t1", 5, "terminal 5", &NewPlace::NewGroup, ());
        assert!(workspace.detached().is_empty(), "tabs of a dead daemon are gone with their marks");
    }

    #[test]
    fn test_c_d85_a_detached_tab_is_hidden_from_a_split_strip_and_dimmed_in_a_single_column() {
        let mut workspace = workspace(&[1, 2]);
        workspace.add_split(DAEMON, 3, "terminal 3", &NewPlace::Group(1), (), Orientation::Horizontal);
        // 칸이 둘이면 열도 둘이고, 분리한 탭은 어느 열에도 없다.
        workspace.detach_terminal(1).unwrap();
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(2, false)], vec![(3, false)]]);
        // 칸을 하나로 합치면(3을 닫는다) 같은 탭이 흐린 탭으로 줄 끝에 남는다.
        workspace.close(3);
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(2, false), (1, true)]]);
        // 묶음의 탭이 모두 다른 창에 있으면 흐린 탭만 남는다.
        workspace.detach_terminal(2).unwrap();
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(1, true), (2, true)]]);
        workspace.reattach_terminal(1, None);
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(1, false), (2, true)]]);
        assert_eq!(workspace.selected(), Some(1), "the selection moves to the tab that came back");
    }

    #[test]
    fn test_c_d82_reattaching_moves_a_selection_that_sits_on_a_detached_tab_to_the_returned_tab() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.close(3);
        workspace.select(2);
        workspace.detach_terminal(1).unwrap();
        workspace.detach_terminal(2).unwrap();
        assert_eq!(workspace.selected(), Some(2));
        workspace.reattach_terminal(1, None);
        assert_eq!(workspace.selected(), Some(1));
    }

    #[test]
    fn test_c_d82_restoring_a_selection_on_a_detached_tab_picks_an_attached_tab_of_the_group() {
        let mut workspace = workspace(&[1, 2]);
        workspace.detach_terminal(1).unwrap();
        workspace.select_or_first(Some(1));
        assert_eq!(workspace.selected(), Some(2));
    }

    #[test]
    fn test_c_d85_faded_tabs_trail_in_tab_list_order_not_in_detach_order() {
        let mut workspace = workspace(&[1, 2, 3]);
        workspace.detach_terminal(2).unwrap();
        workspace.detach_terminal(1).unwrap();
        assert_eq!(workspace.view_detached_ids(), vec![1, 2]);
        workspace.reattach_terminal(1, None);
        workspace.detach_terminal(3).unwrap();
        assert_eq!(workspace.strip_columns(1000.0, 600.0), vec![vec![(1, false), (2, true), (3, true)]]);
    }

    /// 묶음 [3, 4]의 파일 칸에 a.txt·b.md·c.png가 열려 있고 폴더 /repo(터미널 1·2)에도 readme.md가 열린 작업 공간.
    fn with_open_files() -> Workspace<()> {
        let mut workspace = two_groups();
        workspace.open_file("/tmp/a.txt", FileKind::Text);
        workspace.open_file("/tmp/b.md", FileKind::Markdown);
        workspace.open_file("/tmp/c.png", FileKind::Image);
        workspace.select(1);
        workspace.open_file("/repo/readme.md", FileKind::Markdown);
        workspace.select(4);
        workspace
    }

    fn shown_files(workspace: &Workspace<()>) -> Vec<String> {
        workspace.file_pane().map(|pane| pane.tabs.iter().map(|tab| tab.path.clone()).collect()).unwrap_or_default()
    }

    #[test]
    fn test_c_d86_a_detached_file_stays_in_the_content_maps_and_unsaved_set() {
        let mut workspace = with_open_files();
        let group = GroupKey::Group(3);
        let detached = workspace.detach_file(&group, 1).unwrap();
        assert_eq!((detached.group.clone(), detached.tab.path.as_str()), (group.clone(), "/tmp/b.md"));
        // 칸에서는 빠지고(선택은 그 자리에 들어온 탭), 어느 묶음에서든 열린 탭 열쇠에는 남는다 — 내용 맵·저장하지 않은 표시를 비우는 기준이다.
        assert_eq!(shown_files(&workspace), ["/tmp/a.txt", "/tmp/c.png"]);
        assert_eq!(workspace.file_pane().unwrap().selected, 1);
        assert!(workspace.open_file_keys().any(|key| key == "/tmp/b.md"));
        assert!(workspace.is_file_detached("/tmp/b.md") && !workspace.is_file_detached("/tmp/a.txt"));
        assert!(workspace.find_file_tab("/tmp/b.md").is_none(), "a detached tab is in no pane");
        assert_eq!(workspace.find_file_tab("/tmp/c.png"), Some((group.clone(), 1)));
        // 칸이 비어도 분리한 탭은 남고, 마지막 탭을 빼면 칸이 사라진다.
        workspace.detach_file(&group, 0).unwrap();
        workspace.detach_file(&group, 0).unwrap();
        assert!(workspace.file_pane().is_none());
        assert_eq!(workspace.detached_files().len(), 3);
        assert!(workspace.detach_file(&group, 0).is_none());
        // 저장 상태에는 되돌아올 묶음의 파일 칸에 섞어 적는다(분리 창은 되살리지 않는다).
        let state = workspace.to_state();
        let saved: Vec<&str> = state.files.get(&group).unwrap().tabs.iter().map(|tab| tab.path.as_str()).collect();
        assert_eq!(saved, ["/tmp/b.md", "/tmp/a.txt", "/tmp/c.png"]);
        // 다른 묶음의 칸은 그대로다.
        assert_eq!(state.files.get(&GroupKey::Folder("/repo".into())).unwrap().tabs.len(), 1);
    }

    #[test]
    fn test_c_d86_reattach_returns_the_file_to_its_groups_pane_end_or_the_current_group() {
        let mut workspace = with_open_files();
        let group = GroupKey::Group(3);
        workspace.detach_file(&group, 0).unwrap();
        // 다른 묶음을 보고 있어도 돌아올 묶음의 칸 끝에 붙는다(그 칸의 탭 하나가 더 열려 있다).
        workspace.select(1);
        assert_eq!(workspace.reattach_file("/tmp/a.txt"), Some((group.clone(), 2)));
        assert!(workspace.detached_files().is_empty());
        assert_eq!(workspace.files().get(&group).unwrap().selected, 2, "the returned tab is selected in its pane");
        workspace.select(4);
        assert_eq!(shown_files(&workspace), ["/tmp/b.md", "/tmp/c.png", "/tmp/a.txt"]);
        // 없는 탭은 아무것도 바꾸지 않는다.
        assert_eq!(workspace.reattach_file("/tmp/nope"), None);
        // 돌아올 묶음이 사라졌으면(터미널을 모두 닫았다) 지금 보는 묶음 칸의 끝에 붙는다.
        workspace.detach_file(&group, 0).unwrap();
        workspace.close(3);
        workspace.close(4);
        assert_eq!(workspace.view_key(), Some(GroupKey::Folder("/repo".into())));
        assert_eq!(workspace.detached_files()[0].group, GroupKey::Group(0), "the vanished group is forgotten");
        assert_eq!(workspace.reattach_file("/tmp/b.md"), Some((GroupKey::Folder("/repo".into()), 1)));
        assert_eq!(shown_files(&workspace), ["/repo/readme.md", "/tmp/b.md"]);
        // 붙일 묶음이 하나도 없으면 탭이 그대로 남는다.
        workspace.detach_file(&GroupKey::Folder("/repo".into()), 0).unwrap();
        workspace.close(1);
        workspace.close(2);
        workspace.remove_folder("/repo");
        assert_eq!(workspace.reattach_file("/repo/readme.md"), None);
        assert_eq!(workspace.detached_files().len(), 1);
    }

    #[test]
    fn test_c_d86_switching_groups_does_not_touch_detached_windows() {
        let mut workspace = with_open_files();
        let group = GroupKey::Group(3);
        workspace.detach_file(&group, 1).unwrap();
        let before = workspace.detached_files().to_vec();
        // 묶음을 바꾸고(폴더 묶음 → Unassigned 묶음 → 폴더 묶음) 새 터미널을 열어도 분리한 탭은 그대로이고, 보는 칸에는 나타나지 않는다.
        workspace.select(1);
        assert_eq!(shown_files(&workspace), ["/repo/readme.md"]);
        workspace.add(DAEMON, 9, "terminal 9", &NewPlace::Group(3), ());
        workspace.select(4);
        assert_eq!(shown_files(&workspace), ["/tmp/a.txt", "/tmp/c.png"]);
        assert_eq!(workspace.detached_files(), before);
        // 터미널 번호를 새로 매겨도(계정 전환) 돌아올 묶음이 따라간다.
        workspace.replace_terminal(3, 30, DAEMON, ());
        assert_eq!(workspace.detached_files()[0].group, GroupKey::Group(30));
        assert!(workspace.reattach_file("/tmp/b.md").is_some());
        assert_eq!(workspace.files().get(&GroupKey::Group(30)).unwrap().tabs.len(), 3);
    }

    #[test]
    fn test_c_d86_a_detached_file_whose_group_vanished_is_saved_into_the_shown_group_not_a_dead_one() {
        let mut workspace = with_open_files();
        let group = GroupKey::Group(3);
        workspace.detach_file(&group, 0).unwrap();
        workspace.close(3);
        workspace.close(4);
        assert_eq!(workspace.detached_files()[0].group, GroupKey::Group(0), "the group is gone");
        let state = workspace.to_state();
        assert!(state.files.get(&GroupKey::Group(0)).is_none(), "no pane under the dead group");
        let saved: Vec<&str> = state
            .files
            .get(&GroupKey::Folder("/repo".into()))
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.path.as_str())
            .collect();
        assert_eq!(saved, ["/repo/readme.md", "/tmp/a.txt"], "the tab comes back in the shown group");
        // 터미널이 하나도 없으면 복원할 곳이 없어 적지 않는다.
        workspace.close(1);
        workspace.close(2);
        assert!(workspace.to_state().files.get(&GroupKey::Group(0)).is_none());
    }
}
