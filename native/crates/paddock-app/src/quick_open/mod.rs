//! 파일 빠른 열기(Ctrl+Shift+P): 파일 구획 폴더(없으면 선택한 터미널의 폴더)의 파일을 이름으로 골라 파일 탭으로 연다.
//! 색인은 `index`의 작업 스레드가, 점수·순서·한도는 `core::quick_open`이, 그리기는 `view`가 맡는다. 열 때마다 색인을 새로 만들고 닫으면 버린다.
#[cfg(feature = "gui")]
pub mod actions;
#[cfg(all(feature = "gui", feature = "automation"))]
pub mod automation;
#[cfg(feature = "gui")]
pub mod view;

pub mod index;

use std::sync::mpsc::TryRecvError;

#[cfg(feature = "gui")]
use gpui_kit::component::input::InputState;
#[cfg(feature = "gui")]
use gpui_kit::{Entity, ScrollHandle};
use paddock_core::menu_nav::{MenuNav, NavKey};
use paddock_core::quick_open::{LIMIT, rank};

use index::{IndexEvent, IndexJob};

/// 열려 있는 빠른 열기 하나.
pub struct QuickOpenState {
    /// 색인한 폴더(파일 경로의 앞부분).
    pub folder: String,
    #[cfg(feature = "gui")]
    pub input: Entity<InputState>,
    /// 지금까지 모은 상대 경로(구분자 `/`).
    pub files: Vec<String>,
    /// 색인이 상한에 닿아 더 담지 않았다.
    pub capped: bool,
    /// 색인이 도는 중이다.
    pub indexing: bool,
    job: Option<IndexJob>,
    /// 마지막으로 순위를 매긴 질의.
    query: String,
    /// 보이는 줄(`files`의 번호, 점수 순).
    ranked: Vec<usize>,
    pub nav: MenuNav,
    /// 목록 스크롤. 강조가 보이는 줄 밖으로 나가면 따라가게 한다.
    #[cfg(feature = "gui")]
    pub scroll: ScrollHandle,
}

impl QuickOpenState {
    #[cfg(feature = "gui")]
    pub fn new(
        folder: String,
        input: Entity<InputState>,
        job: IndexJob,
    ) -> Self {
        Self {
            folder,
            input,
            files: Vec::new(),
            capped: false,
            indexing: true,
            job: Some(job),
            query: String::new(),
            ranked: Vec::new(),
            nav: MenuNav::default(),
            scroll: ScrollHandle::new(),
        }
    }

    /// 보이는 줄들의 상대 경로(점수 순).
    pub fn shown(&self) -> impl Iterator<Item = &String> {
        self.ranked.iter().filter_map(|index| self.files.get(*index))
    }

    pub fn n_shown(&self) -> usize {
        self.ranked.len()
    }

    /// 강조한 줄의 상대 경로.
    pub fn chosen(&self) -> Option<&String> {
        self.nav.chosen(self.ranked.len()).and_then(|at| self.ranked.get(at)).and_then(|index| self.files.get(*index))
    }

    /// 작업 스레드가 보낸 소식을 모두 반영하고 순위를 다시 매긴다. 화면에 보이는 것이 바뀌었으면 true다.
    pub fn pump(
        &mut self,
        query: &str,
    ) -> bool {
        let mut changed = false;
        let mut ended = false;
        while let Some(job) = self.job.as_ref()
            && !ended
        {
            match job.try_recv() {
                Ok(IndexEvent::Files(mut more)) => {
                    self.files.append(&mut more);
                    changed = true;
                }
                Ok(IndexEvent::Done { capped }) => {
                    self.capped = capped;
                    ended = true;
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    changed = true;
                }
            }
        }
        if ended {
            self.job = None;
            self.indexing = false;
        }
        let query_changed = query != self.query;
        if changed || query_changed {
            self.query = query.to_string();
            self.ranked = rank(query, &self.files, LIMIT);
            // 질의가 바뀌면 강조를 첫 줄로 돌린다. 색인이 늘어 목록만 바뀐 때는 사용자가 옮긴 강조를 지킨다(줄 수를 넘으면 마지막 줄).
            let kept =
                self.nav.highlight.filter(|_| !query_changed).map(|at| at.min(self.ranked.len().saturating_sub(1)));
            self.nav.highlight = if self.ranked.is_empty() { None } else { Some(kept.unwrap_or(0)) };
            changed = true;
        }
        changed
    }

    /// ↑·↓·Home·End로 강조를 옮긴다.
    pub fn step(
        &mut self,
        key: NavKey,
    ) {
        self.nav.step(key, self.ranked.len());
    }
}
