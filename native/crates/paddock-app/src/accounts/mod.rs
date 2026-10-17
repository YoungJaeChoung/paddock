//! 계정 터미널: 등록 계정 목록 파일 읽기·쓰기, ＋ 메뉴·계정 고르기의 항목 구성, Accounts 창, 계정 터미널 실행 명세 만들기.
//!
//! 규칙(검사·실행 명세)은 core의 `account_profile`·`account_launch`가 쥐고, 이 모듈은 파일과 화면을 다룬다.
//! 항목·문구를 정하는 순수 부분은 항상 빌드되고, 그리기는 `gui` 기능일 때만 빌드된다.

#[cfg(feature = "gui")]
pub mod dialog;
pub mod launch;
pub mod model;
pub mod store;
#[cfg(feature = "gui")]
pub mod window;
