//! 상태 줄 사용량: Claude·Codex 사용량 읽기(작업 스레드), 읽기 상태, 상태 줄 묶음 그리기.
//!
//! 규칙은 core의 `usage_model`·`usage_scopes`·`codex_usage`가 쥐고, 여기는 파일·프로세스를 읽고 그 결과를 창에 잇는다.
//! 실제 계정 자료(계정 폴더·로그인 정보 파일)는 읽기만 한다. 쓰는 것은 Paddock 전용 Claude 설정(`setup`)뿐이다.
#[cfg(feature = "gui")]
pub mod bar;
pub mod codex;
pub mod data;
pub mod files;
#[cfg(feature = "gui")]
pub mod launcher_dialog;
pub mod login;
pub mod memory;
#[cfg(feature = "gui")]
pub mod popup;
pub mod setup;
pub mod wsl;
