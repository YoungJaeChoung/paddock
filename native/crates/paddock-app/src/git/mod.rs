//! Git 연동: 프로세스 실행(`runner`), 저장소 읽기·쓰기(`status`), 실행 시점 규칙(`gate`), 작업 스레드(`service`), 창이 쥐는 상태(`state`).
pub mod gate;
pub mod runner;
pub mod service;
pub mod state;
pub mod status;
