//! Source control: 사이드바 보기(변경 목록·커밋 입력칸), 변경 비교 탭, 스테이징·커밋·되돌리기 동작.
//!
//! 규칙은 `model`(창 없이 시험), 그리기는 `view`·`diff_view`, 창을 움직이는 동작은 `actions`에 있다.
use std::collections::HashSet;

#[cfg(feature = "gui")]
use gpui_kit::Entity;
#[cfg(feature = "gui")]
use gpui_kit::component::input::TextareaState;

#[cfg(feature = "gui")]
pub mod actions;
#[cfg(feature = "gui")]
pub mod diff_view;
pub mod model;
#[cfg(feature = "gui")]
pub mod view;

use model::Group;

/// 사이드바가 지금 보여 주는 보기.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarView {
    Work,
    SourceControl,
}

impl SidebarView {
    /// 상태 글과 대본에서 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::SourceControl => "scm",
        }
    }
}

/// 창이 쥐는 Source control 상태.
pub struct ScmState {
    pub view: SidebarView,
    /// 커밋 메시지 입력칸. Source control 보기를 처음 열 때 만든다.
    #[cfg(feature = "gui")]
    pub commit_input: Option<Entity<TextareaState>>,
    /// 접은 구획.
    pub collapsed: HashSet<Group>,
    /// 입력칸 아래 한 줄: 커밋 실패 이유나 메시지 요구.
    pub note: Option<String>,
    /// 열린 비교 탭을 마지막으로 맞춘 Git 상태 번호.
    pub seen_generation: u64,
    /// 터미널마다 마지막으로 본 출력 시각. 바뀌었을 때만 출력 사건으로 센다.
    pub output: crate::git::gate::OutputWatch,
    /// 지난 확인 때 창이 활성이었다. 비활성에서 활성으로 돌아올 때를 알아내는 데 쓴다.
    pub was_active: bool,
}

impl Default for ScmState {
    fn default() -> Self {
        Self {
            view: SidebarView::Work,
            #[cfg(feature = "gui")]
            commit_input: None,
            collapsed: HashSet::new(),
            note: None,
            seen_generation: 0,
            output: crate::git::gate::OutputWatch::default(),
            was_active: true,
        }
    }
}
