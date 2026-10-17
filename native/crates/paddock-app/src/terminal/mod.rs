//! 터미널 하나를 앱 안에서 다루는 부분: 화면 격자, 데몬 세션, 색 규칙, 그리기와 입력.

pub mod banner;
pub mod grid;
pub mod ime;
#[cfg(feature = "gui")]
pub mod paint;
pub mod palette;
pub mod session;
pub mod snapshot;
#[cfg(feature = "gui")]
pub mod view;
