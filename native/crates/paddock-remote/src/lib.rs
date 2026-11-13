//! 원격 게이트웨이: 브라우저가 접속하는 HTTP·WebSocket 서버와 그 인증 계층.
//!
//! 127.0.0.1에만 열고, 밖에서 오는 연결은 `tailscale serve`가 받아 넘긴다. 요청은 게이트(`gate`)의 검사를 지나야 하고,
//! 기기는 맥에서 연 승인 창의 1회용 코드로만 페어링한다(`pairing`·`devices`). 앱은 제어 소켓(`control`)으로 게이트웨이를 다룬다.
pub mod connections;
pub mod control;
pub mod daemon_link;
pub mod devices;
pub mod gate;
pub mod gateway;
pub mod http;
pub mod log;
pub mod pairing;
pub mod websocket;
