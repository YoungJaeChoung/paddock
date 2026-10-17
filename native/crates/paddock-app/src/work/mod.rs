//! Work 목록: 작업 폴더와 Unassigned 묶음별로 묶인 터미널 목록, 우클릭·환경 고르기 메뉴, 이름 바꾸기 창, 첫 실행 안내.
//!
//! 항목·문구를 정하는 순수 규칙(`menu_model`, `empty_model`)은 항상 빌드되고, 그리기는 `gui` 기능일 때만 빌드된다.

pub mod empty_model;
#[cfg(feature = "gui")]
pub mod empty_state;
#[cfg(feature = "gui")]
pub mod folder_row;
#[cfg(feature = "gui")]
pub mod list_view;
#[cfg(feature = "gui")]
pub mod menu;
pub mod menu_model;
#[cfg(feature = "gui")]
pub mod rename_dialog;
#[cfg(feature = "gui")]
pub mod terminal_row;
