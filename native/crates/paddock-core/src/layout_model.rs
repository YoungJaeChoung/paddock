//! 묶음의 본문 칸 배치(분할)를 터미널 번호로 나타내는 순수 모델: 칸 트리, 정리, 저장·복원, 분할, 칸 크기.
//!
//! 한 묶음(작업 폴더 또는 Unassigned 묶음)의 화면은 칸(`Pane`) 하나이거나, 가로·세로로 나뉜 칸들(`Split`)이다. 칸은 탭으로
//! 터미널 여럿을 쥐고 그중 하나를 보여 준다. 여러 묶음의 배치를 `besides`로 한 트리에 이었다가 `prune`으로 묶음별로 다시 나눌 수 있다.
//! 화면 그리기와 저장소를 모른다. 터미널의 실제 수명은 데몬이 쥐므로 이 모델은 번호만 다룬다.
use std::collections::HashSet;

use serde_json::{Value, json};

/// 칸이 나뉜 방향.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// 칸들이 왼쪽에서 오른쪽으로 놓인다(옆 칸).
    Horizontal,
    /// 칸들이 위에서 아래로 놓인다(아래 칸).
    Vertical,
}

impl Orientation {
    fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    fn from_name(name: Option<&str>) -> Option<Self> {
        match name {
            Some("horizontal") => Some(Self::Horizontal),
            Some("vertical") => Some(Self::Vertical),
            _ => None,
        }
    }
}

/// 칸 트리. 칸의 `selected`는 `tabs` 안의 번호이고, `Split`의 `sizes`는 자식마다 하나인 상대 비율이다(합이 1일 필요는 없다).
#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane { tabs: Vec<u32>, selected: usize },
    Split { orientation: Orientation, children: Vec<Layout>, sizes: Vec<f64> },
}

/// 화면 크기를 `Layout`에 나눠 준 칸 하나의 위치와 크기.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneBox {
    /// 왼쪽·위부터 센 칸 번호(`terminal_ids`의 순서와 같다).
    pub index: usize,
    pub tabs: Vec<u32>,
    pub selected: usize,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// 저장값의 `sizes` 한 항목을 읽는다. 유한하고 0 이상인 숫자만 쓰고 나머지는 1이다.
fn saved_size(
    sizes: Option<&Value>,
    index: usize,
) -> f64 {
    sizes
        .and_then(Value::as_array)
        .and_then(|list| list.get(index))
        .and_then(Value::as_f64)
        .filter(|size| size.is_finite() && *size >= 0.0)
        .unwrap_or(1.0)
}

impl Layout {
    /// 탭 `tabs` 중 `selected`번째를 보여 주는 칸.
    pub fn pane(
        tabs: Vec<u32>,
        selected: usize,
    ) -> Self {
        Self::Pane { tabs, selected }
    }

    /// 배치에 든 터미널을 왼쪽·위부터 순서대로 돌려준다.
    pub fn terminal_ids(&self) -> Vec<u32> {
        match self {
            Self::Pane { tabs, .. } => tabs.clone(),
            Self::Split { children, .. } => children.iter().flat_map(Self::terminal_ids).collect(),
        }
    }

    /// 배치에 터미널이 들어 있는지.
    pub fn includes(
        &self,
        id: u32,
    ) -> bool {
        self.terminal_ids().contains(&id)
    }

    /// `keep`이 참인 터미널만 남긴 배치. 남은 터미널이 없는 칸은 사라지고, 자식이 하나만 남은 분할은 그 자식이 된다.
    ///
    /// 칸에서 선택된 탭은 남은 탭 중 원래 선택한 것을 유지하고, 사라졌으면 같은 번호(넘으면 마지막)의 탭으로 옮긴다.
    /// 분할에서 사라진 칸의 비율은 함께 빠지고 남은 칸끼리 상대 비율이 유지된다. 남은 터미널이 없으면 None이다.
    ///
    /// Examples
    /// --------
    /// | 배치                         | keep      | 결과                    |
    /// |------------------------------|-----------|-------------------------|
    /// | 칸 [1, 9, 2] 선택 1          | 9 제외    | 칸 [1, 2] 선택 1        |
    /// | 가로 [칸 [1], 칸 [9]]        | 1만       | 칸 [1]                  |
    /// | 칸 [9]                       | 없음      | None                    |
    pub fn prune(
        &self,
        keep: impl Fn(u32) -> bool,
    ) -> Option<Layout> {
        self.pruned(&keep)
    }

    fn pruned(
        &self,
        keep: &dyn Fn(u32) -> bool,
    ) -> Option<Layout> {
        match self {
            Self::Pane { tabs, selected } => {
                let kept: Vec<u32> = tabs.iter().copied().filter(|id| keep(*id)).collect();
                if kept.is_empty() {
                    None
                } else {
                    let position = tabs.get(*selected).and_then(|chosen| kept.iter().position(|id| id == chosen));
                    let selected = position.unwrap_or((*selected).min(kept.len() - 1));
                    Some(Self::Pane { tabs: kept, selected })
                }
            }
            Self::Split { orientation, children, sizes } => {
                let mut kept_children = Vec::new();
                let mut kept_sizes = Vec::new();
                for (index, child) in children.iter().enumerate() {
                    if let Some(pruned) = child.pruned(keep) {
                        kept_children.push(pruned);
                        kept_sizes.push(sizes.get(index).copied().unwrap_or(1.0));
                    }
                }
                match kept_children.len() {
                    0 => None,
                    1 => kept_children.pop(),
                    _ => Some(Self::Split { orientation: *orientation, children: kept_children, sizes: kept_sizes }),
                }
            }
        }
    }

    /// 터미널 번호를 `new_id`가 알려 주는 번호로 바꾼 배치. 새 번호가 없는 터미널(`None`)은 `prune`처럼 뺀다.
    ///
    /// 터미널을 되살리면 데몬이 새 번호를 매기므로, 저장된 배치를 새 번호로 옮기는 데 쓴다. 칸 구조·선택·비율은 그대로다.
    ///
    /// Examples
    /// --------
    /// | 배치                    | 번호 바꿈       | 결과                  |
    /// |-------------------------|-----------------|-----------------------|
    /// | 가로 [칸 [1, 2], 칸 [3]] | 1→10, 2→20, 3→30 | 가로 [칸 [10, 20], 칸 [30]] |
    /// | 칸 [1, 2]               | 1→10            | 칸 [10]               |
    /// | 칸 [1]                  | (없음)          | None                  |
    pub fn renumbered(
        &self,
        new_id: impl Fn(u32) -> Option<u32>,
    ) -> Option<Layout> {
        self.prune(|id| new_id(id).is_some()).map(|kept| kept.with_ids_mapped(&new_id))
    }

    fn with_ids_mapped(
        self,
        new_id: &dyn Fn(u32) -> Option<u32>,
    ) -> Layout {
        match self {
            Self::Pane { tabs, selected } => {
                Self::Pane { tabs: tabs.into_iter().filter_map(new_id).collect(), selected }
            }
            Self::Split { orientation, children, sizes } => Self::Split {
                orientation,
                children: children.into_iter().map(|child| child.with_ids_mapped(new_id)).collect(),
                sizes,
            },
        }
    }

    /// 열려 있는 터미널(`open`)만 남긴 배치. 닫힌 터미널을 뺀다.
    pub fn without_closed(
        &self,
        open: &HashSet<u32>,
    ) -> Option<Layout> {
        self.prune(|id| open.contains(&id))
    }

    /// 배치의 첫 번째 칸 끝에 터미널을 탭으로 더하고 그 탭을 선택한다. 배치가 없으면 터미널 하나짜리 칸이다.
    pub fn with_terminal(
        area: Option<&Layout>,
        id: u32,
    ) -> Layout {
        match area {
            None => Self::Pane { tabs: vec![id], selected: 0 },
            Some(Self::Pane { tabs, .. }) => {
                let mut tabs = tabs.clone();
                tabs.push(id);
                Self::Pane { selected: tabs.len() - 1, tabs }
            }
            Some(Self::Split { orientation, children, sizes }) => {
                let mut children = children.clone();
                if let Some(first) = children.first_mut() {
                    *first = Self::with_terminal(Some(first), id);
                }
                Self::Split { orientation: *orientation, children, sizes: sizes.clone() }
            }
        }
    }

    /// `anchor`가 든 칸의 끝에 터미널을 탭으로 더하고 선택한다. `anchor`가 배치에 없으면 첫 칸 끝이다.
    pub fn add_beside(
        &self,
        anchor: u32,
        id: u32,
    ) -> Layout {
        if self.includes(anchor) { self.added_in_pane_of(anchor, id) } else { Self::with_terminal(Some(self), id) }
    }

    fn added_in_pane_of(
        &self,
        anchor: u32,
        id: u32,
    ) -> Layout {
        match self {
            Self::Pane { tabs, .. } if tabs.contains(&anchor) => {
                let mut tabs = tabs.clone();
                tabs.push(id);
                Self::Pane { selected: tabs.len() - 1, tabs }
            }
            Self::Pane { .. } => self.clone(),
            Self::Split { orientation, children, sizes } => Self::Split {
                orientation: *orientation,
                children: children.iter().map(|child| child.added_in_pane_of(anchor, id)).collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// 여러 배치를 가로로 나란히 이은 한 배치. 각 배치의 칸 나눔·칸별 선택 탭은 그대로 남는다.
    ///
    /// 나중에 묶음별로 `prune`하면 각 묶음이 원래 배치로 돌아온다. None은 건너뛰고, 하나만 남으면 그 배치, 없으면 None이다.
    pub fn besides(areas: Vec<Option<Layout>>) -> Option<Layout> {
        let mut children: Vec<Layout> = areas.into_iter().flatten().collect();
        match children.len() {
            0 => None,
            1 => children.pop(),
            n_children => Some(Self::Split {
                orientation: Orientation::Horizontal,
                sizes: vec![1.0 / n_children as f64; n_children],
                children,
            }),
        }
    }

    /// 터미널이 속한 칸에서 그 터미널을 선택된 탭으로 만든다. 배치에 없는 터미널이면 그대로 돌려준다.
    pub fn select(
        &self,
        id: u32,
    ) -> Layout {
        match self {
            Self::Pane { tabs, selected } => {
                Self::Pane { tabs: tabs.clone(), selected: tabs.iter().position(|tab| *tab == id).unwrap_or(*selected) }
            }
            Self::Split { orientation, children, sizes } => Self::Split {
                orientation: *orientation,
                children: children.iter().map(|child| child.select(id)).collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// 새 칸이 들어간 분할의 형제 칸에 같은 공간을 준다.
    ///
    /// 새 터미널이 든 칸의 부모 분할이 `orientation`과 같은 방향일 때만 그 분할의 모든 칸을 같은 비율로 맞춘다.
    /// 다른 방향의 분할과 그 안에서 사용자가 정한 비율·선택 탭은 유지한다. 새 칸을 만든 직후에만 적용하며
    /// 창 크기 변경이나 저장 배치 복원에는 적용하지 않는다.
    pub fn balance_new_split(
        &self,
        id: u32,
        orientation: Orientation,
    ) -> Layout {
        match self {
            Self::Pane { .. } => self.clone(),
            Self::Split { orientation: own, children, sizes } => {
                let holds_new_pane =
                    children.iter().any(|child| matches!(child, Self::Pane { tabs, .. } if tabs.contains(&id)));
                if holds_new_pane && *own == orientation {
                    Self::Split {
                        orientation: *own,
                        children: children.clone(),
                        sizes: vec![1.0 / children.len() as f64; children.len()],
                    }
                } else {
                    Self::Split {
                        orientation: *own,
                        children: children.iter().map(|child| child.balance_new_split(id, orientation)).collect(),
                        sizes: sizes.clone(),
                    }
                }
            }
        }
    }

    /// `anchor`가 든 칸 옆(가로)·아래(세로)에 `id`만 든 새 칸을 만든다. `id`가 다른 칸의 탭이었으면 거기서 뺀다.
    ///
    /// 앵커 칸의 부모가 같은 방향 분할이면 그 분할에 형제 칸으로 끼워 모든 칸이 같은 크기를 갖고, 아니면 앵커 칸을 둘로 나눈다
    /// (반반). `anchor`가 배치에 없거나 `id`와 같으면 그대로다.
    ///
    /// Examples
    /// --------
    /// | 배치                      | 호출                    | 결과                          |
    /// |---------------------------|-------------------------|-------------------------------|
    /// | 칸 [1, 2]                 | split(1, 2, Horizontal) | 가로 [칸 [1], 칸 [2]] 반반    |
    /// | 가로 [칸 [1], 칸 [2]]     | split(2, 3, Horizontal) | 가로 [1, 2, 3] 각 1/3         |
    /// | 가로 [칸 [1], 칸 [2]]     | split(1, 3, Vertical)   | 가로 [세로 [1, 3], 칸 [2]]    |
    pub fn split(
        &self,
        anchor: u32,
        id: u32,
        orientation: Orientation,
    ) -> Layout {
        if anchor == id || !self.includes(anchor) {
            self.clone()
        } else {
            let without = self.prune(|other| other != id).unwrap_or_else(|| self.clone());
            let inserted = without.inserted_pane(anchor, id, orientation);
            inserted.balance_new_split(id, orientation)
        }
    }

    fn inserted_pane(
        &self,
        anchor: u32,
        id: u32,
        orientation: Orientation,
    ) -> Layout {
        let new_pane = Self::Pane { tabs: vec![id], selected: 0 };
        match self {
            Self::Pane { tabs, .. } if tabs.contains(&anchor) => {
                Self::Split { orientation, children: vec![self.clone(), new_pane], sizes: vec![0.5, 0.5] }
            }
            Self::Pane { .. } => self.clone(),
            Self::Split { orientation: own, children, sizes } => {
                let direct = children
                    .iter()
                    .position(|child| matches!(child, Self::Pane { tabs, .. } if tabs.contains(&anchor)));
                match direct {
                    Some(index) if *own == orientation => {
                        let mut children = children.clone();
                        let mut sizes = sizes.clone();
                        children.insert(index + 1, new_pane);
                        sizes.insert(index + 1, 1.0);
                        Self::Split { orientation: *own, children, sizes }
                    }
                    _ => Self::Split {
                        orientation: *own,
                        children: children.iter().map(|child| child.inserted_pane(anchor, id, orientation)).collect(),
                        sizes: sizes.clone(),
                    },
                }
            }
        }
    }

    /// `path`(자식 번호의 열)가 가리키는 분할의 `sizes`를 바꾼다. 경계를 끌어 칸 크기를 바꿀 때 쓴다.
    ///
    /// 경로가 분할이 아닌 곳을 가리키거나 `sizes`의 수가 그 분할의 자식 수와 다르면 그대로 돌려준다.
    pub fn with_sizes(
        &self,
        path: &[usize],
        sizes: Vec<f64>,
    ) -> Layout {
        match (self, path.split_first()) {
            (Self::Split { orientation, children, sizes: own }, None) => Self::Split {
                orientation: *orientation,
                children: children.clone(),
                sizes: if sizes.len() == children.len() { sizes } else { own.clone() },
            },
            (Self::Split { orientation, children, sizes: own }, Some((first, rest))) => {
                let mut children = children.clone();
                if let Some(child) = children.get_mut(*first) {
                    *child = child.with_sizes(rest, sizes);
                }
                Self::Split { orientation: *orientation, children, sizes: own.clone() }
            }
            (Self::Pane { .. }, _) => self.clone(),
        }
    }

    /// `path`가 가리키는 분할의 크기 비율. 분할이 아니거나 경로가 틀리면 None이다.
    pub fn sizes_at(
        &self,
        path: &[usize],
    ) -> Option<Vec<f64>> {
        match (self, path.split_first()) {
            (Self::Split { sizes, .. }, None) => Some(sizes.clone()),
            (Self::Split { children, .. }, Some((first, rest))) => children.get(*first)?.sizes_at(rest),
            (Self::Pane { .. }, _) => None,
        }
    }

    /// 화면 `width`×`height`를 비율대로 나눠 준 칸들의 위치와 크기(왼쪽·위부터 센 순서).
    ///
    /// 칸 사이 선의 두께는 세지 않는다(선은 칸 위에 겹쳐 그린다).
    pub fn pane_boxes(
        &self,
        width: f32,
        height: f32,
    ) -> Vec<PaneBox> {
        let mut boxes = Vec::new();
        self.collect_boxes((0.0, 0.0, width, height), &mut boxes);
        boxes
    }

    fn collect_boxes(
        &self,
        (left, top, width, height): (f32, f32, f32, f32),
        boxes: &mut Vec<PaneBox>,
    ) {
        match self {
            Self::Pane { tabs, selected } => boxes.push(PaneBox {
                index: boxes.len(),
                tabs: tabs.clone(),
                selected: *selected,
                left,
                top,
                width,
                height,
            }),
            Self::Split { orientation, children, sizes } => {
                let total: f64 = sizes.iter().sum();
                let mut offset = 0.0_f64;
                for (index, child) in children.iter().enumerate() {
                    let share = if total > 0.0 {
                        sizes.get(index).copied().unwrap_or(1.0) / total
                    } else {
                        1.0 / children.len() as f64
                    };
                    let (before, span) = (offset as f32, share as f32);
                    match orientation {
                        Orientation::Horizontal => {
                            child.collect_boxes((left + before * width, top, span * width, height), boxes)
                        }
                        Orientation::Vertical => {
                            child.collect_boxes((left, top + before * height, width, span * height), boxes)
                        }
                    }
                    offset += share;
                }
            }
        }
    }

    /// `path`가 가리키는 분할이 차지한 화면 크기(`width`, `height`). 분할이 아니거나 경로가 틀리면 None이다.
    pub fn extent_at(
        &self,
        path: &[usize],
        width: f32,
        height: f32,
    ) -> Option<(f32, f32)> {
        match (self, path.split_first()) {
            (Self::Split { .. }, None) => Some((width, height)),
            (Self::Split { orientation, children, sizes }, Some((first, rest))) => {
                let total: f64 = sizes.iter().sum();
                let share = if total > 0.0 {
                    sizes.get(*first).copied().unwrap_or(1.0) / total
                } else {
                    1.0 / children.len() as f64
                };
                let child = children.get(*first)?;
                match orientation {
                    Orientation::Horizontal => child.extent_at(rest, share as f32 * width, height),
                    Orientation::Vertical => child.extent_at(rest, width, share as f32 * height),
                }
            }
            (Self::Pane { .. }, _) => None,
        }
    }

    /// 배치를 JSON으로 저장할 수 있는 값으로 바꾼다. 빈 배치(None)는 null이다.
    ///
    /// 분할 방향·크기 비율·선택 탭을 보존해, 종료 때 실제 칸 크기가 달라져도 원래 배치를 복원할 수 있다.
    pub fn serialize(area: Option<&Layout>) -> Value {
        match area {
            None => Value::Null,
            Some(Self::Pane { tabs, selected }) => json!({"type": "pane", "tabs": tabs, "selected": selected}),
            Some(Self::Split { orientation, children, sizes }) => json!({
                "type": "split",
                "orientation": orientation.name(),
                "children": children.iter().map(|child| Self::serialize(Some(child))).collect::<Vec<_>>(),
                "sizes": sizes,
            }),
        }
    }

    /// 저장한 값을 `alive`가 살아 있다고 답한 터미널로 복원한다.
    ///
    /// 살아 있지 않은 터미널·중복 터미널·모양이 틀린 칸은 뺀다. 중복은 처음 나온 자리(왼쪽·위)에만 남긴다. 선택 탭과 남은
    /// 칸의 상대 비율은 `prune`처럼 유지한다. 음수·숫자가 아닌 비율은 1로, 틀린 선택 번호는 0으로 바꾼다.
    /// 저장값은 바꾸지 않으며 복원할 터미널이 없으면 None이다.
    ///
    /// Examples
    /// --------
    /// | 저장값                                          | 결과                      |
    /// |-------------------------------------------------|---------------------------|
    /// | 칸 tabs [99, 1], 99는 죽음                      | 칸 [1]                    |
    /// | 칸 tabs [1, 2, 1] 선택 2                        | 칸 [1, 2] 선택 0          |
    /// | `{"type": "unknown"}`·숫자·null                 | None                      |
    pub fn restore(
        saved: &Value,
        alive: impl Fn(u32) -> bool,
    ) -> Option<Layout> {
        let mut seen = HashSet::new();
        Self::restored(saved, &alive, &mut seen)
    }

    fn restored(
        saved: &Value,
        alive: &dyn Fn(u32) -> bool,
        seen: &mut HashSet<u32>,
    ) -> Option<Layout> {
        let kind = saved.get("type").and_then(Value::as_str);
        match kind {
            Some("pane") => {
                let entries = saved.get("tabs").and_then(Value::as_array);
                entries.and_then(|entries| {
                    let resolved: Vec<Option<u32>> = entries
                        .iter()
                        .map(|entry| entry.as_u64().and_then(|id| u32::try_from(id).ok()).filter(|id| alive(*id)))
                        .collect();
                    let requested = saved.get("selected").and_then(Value::as_u64).map_or(0, |index| index as usize);
                    let chosen = resolved.get(requested).copied().flatten();
                    let mut tabs = Vec::new();
                    for id in resolved.into_iter().flatten() {
                        if seen.insert(id) {
                            tabs.push(id);
                        }
                    }
                    let found = chosen.and_then(|chosen| tabs.iter().position(|id| *id == chosen));
                    (!tabs.is_empty()).then(|| {
                        let selected = found.unwrap_or(requested.min(tabs.len() - 1));
                        Self::Pane { tabs, selected }
                    })
                })
            }
            Some("split") => {
                let orientation = Orientation::from_name(saved.get("orientation").and_then(Value::as_str));
                let entries = saved.get("children").and_then(Value::as_array);
                match (orientation, entries) {
                    (Some(orientation), Some(entries)) => {
                        let mut children = Vec::new();
                        let mut sizes = Vec::new();
                        for (index, entry) in entries.iter().enumerate() {
                            if let Some(child) = Self::restored(entry, alive, seen) {
                                children.push(child);
                                sizes.push(saved_size(saved.get("sizes"), index));
                            }
                        }
                        match children.len() {
                            0 => None,
                            1 => children.pop(),
                            _ => Some(Self::Split { orientation, children, sizes }),
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// 칸 사이 경계를 끌었을 때의 새 `sizes`. `index`번째와 `index + 1`번째 칸이 크기를 주고받고 나머지는 그대로다.
///
/// `delta_fraction`은 끈 거리를 분할 전체 길이로 나눈 값(오른쪽·아래로 끌면 양수)이다. 두 칸의 합과 전체 합은 변하지 않고,
/// 어느 칸도 전체의 `min_fraction` 밑으로 줄지 않는다. 경계가 없는 `index`면 그대로 돌려준다.
///
/// Examples
/// --------
/// | sizes        | index | delta | min | 결과              |
/// |--------------|-------|-------|-----|-------------------|
/// | 1, 1, 2      | 0     | 0.125 | 0.1 | 1.5, 0.5, 2       |
/// | 1, 1, 2      | 0     | 5.0   | 0.1 | 1.6, 0.4, 2       |
/// | 1, 1, 2      | 2     | 0.1   | 0.1 | 1, 1, 2 (경계 없음) |
pub fn drag_boundary(
    sizes: &[f64],
    index: usize,
    delta_fraction: f64,
    min_fraction: f64,
) -> Vec<f64> {
    let mut result = sizes.to_vec();
    if let (Some(before), Some(after)) = (sizes.get(index).copied(), sizes.get(index + 1).copied()) {
        let total: f64 = sizes.iter().sum();
        let pair = before + after;
        let least = (total * min_fraction).min(pair / 2.0);
        let moved = (before + delta_fraction * total).clamp(least, pair - least);
        result[index] = moved;
        result[index + 1] = pair - moved;
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::{Value, json};

    use super::*;

    fn pane(
        tabs: &[u32],
        selected: usize,
    ) -> Layout {
        Layout::Pane { tabs: tabs.to_vec(), selected }
    }

    fn split(
        children: Vec<Layout>,
        orientation: Orientation,
    ) -> Layout {
        let sizes = children.iter().map(|_| 1.0).collect();
        Layout::Split { orientation, children, sizes }
    }

    fn split_sized(
        children: Vec<Layout>,
        sizes: &[f64],
        orientation: Orientation,
    ) -> Layout {
        Layout::Split { orientation, children, sizes: sizes.to_vec() }
    }

    fn row(children: Vec<Layout>) -> Layout {
        split(children, Orientation::Horizontal)
    }

    fn column(children: Vec<Layout>) -> Layout {
        split(children, Orientation::Vertical)
    }

    fn sizes_of(layout: &Layout) -> Vec<f64> {
        match layout {
            Layout::Split { sizes, .. } => sizes.clone(),
            Layout::Pane { .. } => Vec::new(),
        }
    }

    fn children_of(layout: &Layout) -> Vec<Layout> {
        match layout {
            Layout::Split { children, .. } => children.clone(),
            Layout::Pane { .. } => Vec::new(),
        }
    }

    fn selected_of(layout: &Layout) -> usize {
        match layout {
            Layout::Pane { selected, .. } => *selected,
            Layout::Split { .. } => usize::MAX,
        }
    }

    fn all_alive(_id: u32) -> bool {
        true
    }

    #[test]
    fn test_c_layout_1_lists_terminals_left_and_top_first() {
        let area = row(vec![pane(&[1, 2], 0), column(vec![pane(&[3], 0), pane(&[4], 0)])]);
        assert_eq!(area.terminal_ids(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_c_layout_2_pruning_drops_empty_panes_and_keeps_the_remaining_size_ratio() {
        let area =
            split_sized(vec![pane(&[1], 0), pane(&[9], 0), pane(&[2], 0)], &[1.0, 2.0, 3.0], Orientation::Horizontal);
        let pruned = area.prune(|id| id != 9).unwrap();
        assert_eq!(pruned.terminal_ids(), vec![1, 2]);
        assert_eq!(sizes_of(&pruned), vec![1.0, 3.0]);
    }

    #[test]
    fn test_c_layout_3_a_split_with_one_child_left_becomes_that_child() {
        let area = row(vec![pane(&[1], 0), pane(&[9], 0)]);
        assert_eq!(area.prune(|id| id == 1), Some(pane(&[1], 0)));
    }

    #[test]
    fn test_c_layout_4_nothing_left_is_none() {
        assert_eq!(row(vec![pane(&[9], 0)]).prune(|_| false), None);
    }

    #[test]
    fn test_c_layout_5_a_vanished_selection_moves_to_the_nearest_tab_and_a_kept_one_stays() {
        let kept = pane(&[1, 9, 2], 1).prune(|id| id != 9).unwrap();
        assert_eq!(selected_of(&kept), 1);
        let moved = pane(&[9, 1, 2], 2).prune(|id| id != 9).unwrap();
        assert_eq!(selected_of(&moved), 1);
        assert_eq!(moved.terminal_ids()[selected_of(&moved)], 2);
        // 선택 번호가 줄어든 탭 수를 넘으면 마지막 탭이다.
        let clamped = pane(&[1, 2, 9], 2).prune(|id| id != 9).unwrap();
        assert_eq!(selected_of(&clamped), 1);
    }

    #[test]
    fn test_c_layout_6_closed_terminals_are_dropped_before_restoring() {
        let area = row(vec![pane(&[1], 0), pane(&[4], 0)]);
        let open: HashSet<u32> = [1].into_iter().collect();
        assert_eq!(area.without_closed(&open).unwrap().terminal_ids(), vec![1]);
    }

    #[test]
    fn test_c_layout_7_a_terminal_joins_the_end_of_the_first_pane_and_is_selected() {
        let area = row(vec![pane(&[1], 0), pane(&[2], 0)]);
        let next = Layout::with_terminal(Some(&area), 7);
        assert_eq!(next.terminal_ids(), vec![1, 7, 2]);
        assert_eq!(selected_of(&children_of(&next)[0]), 1);
        assert_eq!(Layout::with_terminal(None, 7).terminal_ids(), vec![7]);
    }

    #[test]
    fn test_c_layout_8_a_terminal_becomes_the_selected_tab_of_its_pane() {
        let area = row(vec![pane(&[1], 0), pane(&[3, 2], 0)]);
        assert_eq!(selected_of(&children_of(&area.select(2))[1]), 1);
        assert!(area.includes(2));
        assert!(!area.includes(9));
        // 없는 터미널을 고르면 그대로다.
        assert_eq!(area.select(9), area);
    }

    #[test]
    fn test_c_layout_9_merged_layouts_split_back_into_their_own_layouts() {
        let shown = row(vec![pane(&[1], 0), column(vec![pane(&[2], 0), pane(&[3], 0)])]);
        let other = row(vec![pane(&[4], 0), pane(&[5], 0)]);
        let merged = Layout::besides(vec![Some(shown.clone()), None, Some(other.clone())]).unwrap();
        assert_eq!(merged.terminal_ids(), vec![1, 2, 3, 4, 5]);
        assert_eq!(merged.prune(|id| id <= 3), Some(shown));
        assert_eq!(merged.prune(|id| id >= 4), Some(other.clone()));
        assert_eq!(Layout::besides(vec![None, Some(other.clone())]), Some(other));
        assert_eq!(Layout::besides(vec![None]), None);
    }

    #[test]
    fn test_c_layout_10_a_new_horizontal_pane_gives_every_sibling_the_same_width_and_keeps_tabs() {
        let area = split_sized(
            vec![pane(&[1, 2], 1), pane(&[3], 0), pane(&[9], 0)],
            &[0.42, 0.42, 0.16],
            Orientation::Horizontal,
        );
        let balanced = area.balance_new_split(9, Orientation::Horizontal);
        let third = 1.0 / 3.0;
        assert_eq!(sizes_of(&balanced), vec![third, third, third]);
        assert_eq!(children_of(&balanced), children_of(&area));
        assert_eq!(sizes_of(&area), vec![0.42, 0.42, 0.16]);
    }

    #[test]
    fn test_c_layout_11_a_new_vertical_pane_only_shares_height_and_keeps_the_outer_ratio() {
        let inner = split_sized(vec![pane(&[2], 0), pane(&[9], 0)], &[0.8, 0.2], Orientation::Vertical);
        let area = split_sized(vec![pane(&[1], 0), inner], &[0.7, 0.3], Orientation::Horizontal);
        let balanced = area.balance_new_split(9, Orientation::Vertical);
        assert_eq!(sizes_of(&balanced), vec![0.7, 0.3]);
        assert_eq!(sizes_of(&children_of(&balanced)[1]), vec![0.5, 0.5]);
    }

    #[test]
    fn test_c_layout_12_a_missing_terminal_or_other_direction_keeps_the_saved_ratio() {
        let area = split_sized(vec![pane(&[1], 0), pane(&[2], 0)], &[0.6, 0.4], Orientation::Horizontal);
        assert_eq!(area.balance_new_split(8, Orientation::Horizontal), area);
        assert_eq!(area.balance_new_split(1, Orientation::Vertical), area);
    }

    #[test]
    fn test_c_layout_13_serialized_references_hold_only_ids_selection_and_shape() {
        let saved = Layout::serialize(Some(&pane(&[5, 6, 7], 1)));
        assert_eq!(saved, json!({"type": "pane", "tabs": [5, 6, 7], "selected": 1}));
        assert_eq!(Layout::serialize(None), Value::Null);
        let nested = Layout::serialize(Some(&split_sized(
            vec![pane(&[1], 0), pane(&[2], 0)],
            &[0.3, 0.7],
            Orientation::Vertical,
        )));
        assert_eq!(
            nested,
            json!({
                "type": "split",
                "orientation": "vertical",
                "children": [
                    {"type": "pane", "tabs": [1], "selected": 0},
                    {"type": "pane", "tabs": [2], "selected": 0},
                ],
                "sizes": [0.3, 0.7],
            })
        );
    }

    #[test]
    fn test_c_layout_14_save_and_restore_keep_nested_ratios_directions_and_selected_tabs() {
        let area = split_sized(
            vec![
                pane(&[1, 2], 1),
                split_sized(vec![pane(&[3], 0), pane(&[4], 0)], &[0.19, 0.81], Orientation::Vertical),
            ],
            &[0.73, 0.27],
            Orientation::Horizontal,
        );
        let saved: Value = serde_json::from_str(&Layout::serialize(Some(&area)).to_string()).unwrap();
        let original = saved.clone();
        let restored = Layout::restore(&saved, all_alive).unwrap();
        assert_eq!(restored, area);
        assert_eq!(saved, original, "restoring never changes the saved value");
    }

    #[test]
    fn test_c_layout_15_missing_or_closed_panes_drop_their_size_only_and_one_child_splits_collapse() {
        let saved = json!({
            "type": "split", "orientation": "horizontal", "sizes": [0.25, 0.1, 0.65],
            "children": [
                {"type": "pane", "tabs": [1], "selected": 0},
                {"type": "pane", "tabs": [99], "selected": 0},
                {"type": "split", "orientation": "vertical", "sizes": [0.6, 0.4], "children": [
                    {"type": "pane", "tabs": [98], "selected": 0},
                    {"type": "pane", "tabs": [2], "selected": 0},
                ]},
            ],
        });
        let original = saved.clone();
        let restored = Layout::restore(&saved, |id| id < 90).unwrap();
        assert_eq!(restored, split_sized(vec![pane(&[1], 0), pane(&[2], 0)], &[0.25, 0.65], Orientation::Horizontal));
        assert_eq!(saved, original);
        assert_eq!(Layout::restore(&json!({"type": "pane", "tabs": [98, 99], "selected": 0}), |id| id < 90), None);
    }

    #[test]
    fn test_c_layout_16_a_living_selection_keeps_its_place_and_a_vanished_one_follows_the_prune_rule() {
        for selected in 0..=3 {
            let saved = json!({"type": "pane", "tabs": [1, 99, 2, 3], "selected": selected});
            let restored = Layout::restore(&saved, |id| id != 99).unwrap();
            let expected = pane(&[1, 99, 2, 3], selected).prune(|id| id != 99).unwrap();
            assert_eq!(restored, expected, "selected {selected}");
        }
    }

    #[test]
    fn test_c_layout_17_a_terminal_saved_in_two_places_stays_once_in_the_first_pane() {
        let saved = json!({
            "type": "split", "orientation": "horizontal", "sizes": [0.2, 0.3, 0.5],
            "children": [
                {"type": "pane", "tabs": [1, 2, 1], "selected": 2},
                {"type": "pane", "tabs": [1], "selected": 0},
                {"type": "pane", "tabs": [2], "selected": 0},
            ],
        });
        assert_eq!(Layout::restore(&saved, all_alive), Some(pane(&[1, 2], 0)));
    }

    #[test]
    fn test_c_layout_18_the_alive_check_sees_only_saved_ids_and_leaves_the_saved_value_alone() {
        let saved = json!({"type": "pane", "tabs": [7, "x", {"id": 3}], "selected": 0});
        let original = saved.clone();
        let asked = std::cell::RefCell::new(Vec::new());
        let restored = Layout::restore(&saved, |id| {
            asked.borrow_mut().push(id);
            true
        });
        assert_eq!(restored, Some(pane(&[7], 0)));
        assert_eq!(*asked.borrow(), vec![7]);
        assert_eq!(saved, original);
    }

    #[test]
    fn test_c_layout_19_bad_panes_and_references_are_skipped_and_only_bad_sizes_and_selection_become_safe_values() {
        let saved = json!({
            "type": "split", "orientation": "horizontal", "sizes": [0.1, 0.2, -1, 0, 0.7],
            "children": [
                null,
                {"type": "pane", "tabs": "invalid"},
                {"type": "pane", "tabs": [null, 4, {}, 1, "a"], "selected": -1},
                {"type": "pane", "tabs": [2], "selected": "last"},
                {"type": "pane", "tabs": [3], "selected": 900},
            ],
        });
        let original = saved.clone();
        let restored = Layout::restore(&saved, |id| id <= 3).unwrap();
        assert_eq!(
            restored,
            split_sized(vec![pane(&[1], 0), pane(&[2], 0), pane(&[3], 0)], &[1.0, 0.0, 0.7], Orientation::Horizontal)
        );
        assert_eq!(saved, original);
        for sizes in [Value::Null, json!("bad"), json!([null, "x"])] {
            let bad = json!({
                "type": "split", "orientation": "horizontal", "sizes": sizes,
                "children": [{"type": "pane", "tabs": [1], "selected": 0}, {"type": "pane", "tabs": [2], "selected": 0}],
            });
            assert_eq!(sizes_of(&Layout::restore(&bad, all_alive).unwrap()), vec![1.0, 1.0]);
        }
    }

    #[test]
    fn test_c_layout_20_unrestorable_saved_values_are_none_without_a_panic() {
        let unusable = [
            Value::Null,
            json!(7),
            json!("bad"),
            json!([]),
            json!({}),
            json!({"type": "unknown"}),
            json!({"type": "pane"}),
            json!({"type": "split", "orientation": "horizontal", "children": []}),
            json!({"type": "split", "orientation": "horizontal", "children": [], "sizes": []}),
            json!({"type": "split", "orientation": "invalid", "children": [{"type": "pane", "tabs": [1], "selected": 0}], "sizes": [1]}),
        ];
        for saved in unusable {
            assert_eq!(Layout::restore(&saved, |_| panic!("unexpected reference")), None, "{saved}");
        }
        // 살아 있지 않은 id만 든 칸은 사라지고 살아 있는 id는 남는다.
        let mixed = json!({"type": "pane", "tabs": [99, 1], "selected": 0});
        assert_eq!(Layout::restore(&mixed, |id| id == 1), Some(pane(&[1], 0)));
    }

    #[test]
    fn test_split_puts_the_new_terminal_in_a_new_pane_beside_or_below_the_anchor_pane() {
        let single = pane(&[1, 2], 0).split(1, 2, Orientation::Horizontal);
        assert_eq!(single, split_sized(vec![pane(&[1], 0), pane(&[2], 0)], &[0.5, 0.5], Orientation::Horizontal));
        // 같은 방향으로 이미 나뉘어 있으면 형제로 끼워 같은 폭을 준다. 다른 방향 안쪽 비율은 건드리지 않는다.
        let three = single.split(2, 3, Orientation::Horizontal);
        let third = 1.0 / 3.0;
        assert_eq!(sizes_of(&three), vec![third, third, third]);
        assert_eq!(three.terminal_ids(), vec![1, 2, 3]);
        let below = single.split(1, 3, Orientation::Vertical);
        assert_eq!(sizes_of(&below), vec![0.5, 0.5]);
        assert_eq!(children_of(&below)[0].terminal_ids(), vec![1, 3]);
        assert_eq!(sizes_of(&children_of(&below)[0]), vec![0.5, 0.5]);
        // 앵커가 없으면 그대로다.
        assert_eq!(single.split(9, 4, Orientation::Horizontal), single);
    }

    #[test]
    fn test_add_beside_appends_a_tab_to_the_pane_of_the_anchor_and_selects_it() {
        let area = row(vec![pane(&[1], 0), pane(&[2, 3], 0)]);
        let next = area.add_beside(3, 7);
        assert_eq!(children_of(&next)[1], pane(&[2, 3, 7], 2));
        assert_eq!(children_of(&next)[0], pane(&[1], 0));
        // 앵커가 없으면 첫 칸 끝이다.
        assert_eq!(children_of(&area.add_beside(99, 7))[0], pane(&[1, 7], 1));
    }

    #[test]
    fn test_with_sizes_replaces_the_sizes_of_the_split_at_the_path_only() {
        let inner = column(vec![pane(&[2], 0), pane(&[3], 0)]);
        let area = row(vec![pane(&[1], 0), inner]);
        let changed = area.with_sizes(&[1], vec![3.0, 1.0]);
        assert_eq!(sizes_of(&children_of(&changed)[1]), vec![3.0, 1.0]);
        assert_eq!(sizes_of(&changed), vec![1.0, 1.0]);
        let outer = area.with_sizes(&[], vec![1.0, 4.0]);
        assert_eq!(sizes_of(&outer), vec![1.0, 4.0]);
        // 칸 수와 다른 크기 목록·없는 경로는 무시한다.
        assert_eq!(area.with_sizes(&[], vec![1.0]), area);
        assert_eq!(area.with_sizes(&[5], vec![1.0, 1.0]), area);
    }

    #[test]
    fn test_pane_boxes_follow_the_sizes_and_number_panes_left_to_right_top_to_bottom() {
        let area = split_sized(
            vec![pane(&[1], 0), column(vec![pane(&[2], 0), pane(&[3, 4], 1)])],
            &[1.0, 3.0],
            Orientation::Horizontal,
        );
        let boxes = area.pane_boxes(1000.0, 600.0);
        assert_eq!(boxes.len(), 3);
        assert_eq!(
            (boxes[0].index, boxes[0].left, boxes[0].top, boxes[0].width, boxes[0].height),
            (0, 0.0, 0.0, 250.0, 600.0)
        );
        assert_eq!((boxes[1].left, boxes[1].top, boxes[1].width, boxes[1].height), (250.0, 0.0, 750.0, 300.0));
        assert_eq!((boxes[2].left, boxes[2].top), (250.0, 300.0));
        assert_eq!((boxes[2].tabs.clone(), boxes[2].selected), (vec![3, 4], 1));
    }

    #[test]
    fn test_extent_at_is_the_pixel_size_of_the_split_at_a_path() {
        let area = split_sized(
            vec![pane(&[1], 0), column(vec![pane(&[2], 0), pane(&[3], 0)])],
            &[1.0, 3.0],
            Orientation::Horizontal,
        );
        assert_eq!(area.extent_at(&[], 1000.0, 600.0), Some((1000.0, 600.0)));
        assert_eq!(area.extent_at(&[1], 1000.0, 600.0), Some((750.0, 600.0)));
        assert_eq!(area.extent_at(&[0], 1000.0, 600.0), None, "a pane is not a split");
        assert_eq!(area.extent_at(&[7], 1000.0, 600.0), None);
    }

    #[test]
    fn test_dragging_a_boundary_moves_size_between_its_two_neighbours_within_the_minimum() {
        let sizes = [1.0, 1.0, 2.0];
        // 전체 4의 1/8(=0.5)만큼 오른쪽으로 끌면 첫 칸이 늘고 둘째 칸이 준다.
        assert_eq!(drag_boundary(&sizes, 0, 0.125, 0.1), vec![1.5, 0.5, 2.0]);
        // 이웃 칸이 최소 크기 밑으로 줄지 않는다.
        let limited = drag_boundary(&sizes, 0, 5.0, 0.1);
        assert!((limited[1] - 0.4).abs() < 1e-9, "{limited:?}");
        assert!((limited[0] + limited[1] - 2.0).abs() < 1e-9);
        let left = drag_boundary(&sizes, 1, -5.0, 0.1);
        assert!((left[1] - 0.4).abs() < 1e-9, "{left:?}");
        assert_eq!(left[0], 1.0);
        // 없는 경계는 그대로다.
        assert_eq!(drag_boundary(&sizes, 2, 0.1, 0.1), sizes.to_vec());
    }

    #[test]
    fn test_sizes_at_reads_the_sizes_of_the_split_at_a_path() {
        let inner = split_sized(vec![pane(&[2], 0), pane(&[3], 0)], &[1.0, 3.0], Orientation::Vertical);
        let area = split_sized(vec![pane(&[1], 0), inner], &[2.0, 5.0], Orientation::Horizontal);
        assert_eq!(area.sizes_at(&[]), Some(vec![2.0, 5.0]));
        assert_eq!(area.sizes_at(&[1]), Some(vec![1.0, 3.0]));
        assert_eq!(area.sizes_at(&[0]), None);
        assert_eq!(area.sizes_at(&[4]), None);
    }
}
