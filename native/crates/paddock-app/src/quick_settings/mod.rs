//! 빠른 설정 패널: 보기 줄의 톱니를 누르면 열리는 설정 모음.
//!
//! 줄 구성과 선택지를 정하는 순수 규칙(`model`)은 항상 빌드되고, 그리기(`panel`)는 `gui` 기능일 때만 빌드된다.

pub mod model;
#[cfg(feature = "gui")]
pub mod panel;
