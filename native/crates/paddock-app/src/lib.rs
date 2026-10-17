//! Paddock 앱 본체: 데몬 실행·연결, 터미널 세션과 화면 격자, 격자 그리기, Work 목록·탭·종료 묻기·상태 복원.
//!
//! 창 없이 시험할 수 있는 부분(데몬 실행·연결, 세션, 격자, 탭 순서, 저장 상태)은 항상 빌드되고,
//! GPUI로 그리는 부분은 `gui` 기능일 때만 빌드된다.

pub mod accounts;
pub mod agent_view;
pub mod app_log;
#[cfg(feature = "automation")]
pub mod automation;
#[cfg(feature = "gui")]
pub mod chrome;
pub mod daemon_client;
pub mod daemon_launch;
pub mod done_notice;
#[cfg(feature = "gui")]
pub mod done_sound;
pub mod fatal_report;
pub mod files;
pub mod git;
pub mod look;
#[cfg(feature = "gui")]
pub mod panes;
pub mod platform_keys;
pub mod quick_settings;
pub mod quit_policy;
#[cfg(feature = "gui")]
pub mod quit_prompt;
pub mod restore;
pub mod scm;
pub mod settings;
pub mod shell_choice;
pub mod startup;
pub mod state_store;
#[cfg(feature = "gui")]
pub mod tabs;
pub mod terminal;
#[cfg(feature = "gui")]
pub mod titlebar;
#[cfg(feature = "gui")]
pub mod view_bar;
#[cfg(feature = "gui")]
pub mod window_view;
pub mod work;
pub mod workspace;

#[cfg(test)]
pub(crate) mod test_support;
