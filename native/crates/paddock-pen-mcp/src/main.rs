//! Pen 데스크톱 앱 연동 MCP 서버(`pen-open`): `pen_open`·`pen_save`·`pen_close` 도구를 stdin·stdout 줄 단위 JSON-RPC로 낸다.
//!
//! Paddock이 설정 파일(`--mcp-config`)로 Claude에 붙인다. 이 프로그램은 항상 Windows 프로세스로 돌고, WSL 안의 Claude는 interop으로 띄운다.
//! 인자 `--wsl-distribution <이름>`이 있으면 WSL 안의 Claude가 준 경로로 읽는다. 요청마다 스레드를 하나씩 써서, 오래 걸리는 열기가
//! 다른 요청의 응답을 막지 않는다.
// 저장·닫기는 Windows에서만 동작한다. 다른 운영체제 빌드에서는 재시도 규칙·시간 상한·저장 경로 선택·메뉴 항목 판정과 저장·메뉴 조작의
// 순서 규칙이 시험에만 쓰인다.
#[cfg_attr(not(windows), allow(dead_code))]
mod limits;
#[cfg_attr(not(windows), allow(dead_code))]
mod menu;
mod open;
mod pencil;
#[cfg_attr(not(windows), allow(dead_code))]
mod retry;
mod rpc;
#[cfg_attr(not(windows), allow(dead_code))]
mod save;
mod session;
#[cfg_attr(not(windows), allow(dead_code))]
mod titles;
mod tools;

#[cfg(windows)]
mod accessible;
#[cfg(windows)]
mod close;
#[cfg(windows)]
mod windows;

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use rpc::ToolRunner;
use session::Session;
use tools::ToolName;

/// Windows가 아닌 운영체제에서 저장·닫기를 부를 때의 거부 문장.
#[cfg(not(windows))]
const UNSUPPORTED: &str =
    "{tool} works only with the Pen desktop app on Windows. On this OS, save and close in the app yourself.";

/// 저장은 창의 File 메뉴를 접근성 기능(MSAA)으로 고르는 방식이라 Windows에서만 된다.
#[cfg(windows)]
fn run_save(
    session: &Session,
    path: Option<&str>,
) -> Result<String, String> {
    save::pen_save(session, path)
}

#[cfg(not(windows))]
fn run_save(
    _session: &Session,
    _path: Option<&str>,
) -> Result<String, String> {
    Err(UNSUPPORTED.replace("{tool}", ToolName::SAVE))
}

/// 닫기는 창에 닫기 메시지를 보내는 방식이라 Windows에서만 된다.
#[cfg(windows)]
fn run_close(
    session: &Session,
    path: Option<&str>,
    discard: bool,
) -> Result<String, String> {
    close::pen_close(session, path, discard)
}

#[cfg(not(windows))]
fn run_close(
    _session: &Session,
    _path: Option<&str>,
    _discard: bool,
) -> Result<String, String> {
    Err(UNSUPPORTED.replace("{tool}", ToolName::CLOSE))
}

impl ToolRunner for Session {
    fn run(
        &self,
        name: &str,
        arguments: &Value,
    ) -> Result<String, String> {
        let path = arguments.get("path").and_then(Value::as_str);
        match name {
            ToolName::OPEN => open::pen_open(self, path),
            ToolName::SAVE => run_save(self, path),
            _ => run_close(self, path, arguments.get("discard").and_then(Value::as_bool) == Some(true)),
        }
    }
}

fn main() {
    #[cfg(windows)]
    windows::disable_stdio_inheritance();
    let session = Arc::new(Session::from_arguments(std::env::args().skip(1)));
    let output = Arc::new(Mutex::new(std::io::stdout()));
    let mut workers = Vec::new();
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) => {
                let (session, output) = (Arc::clone(&session), Arc::clone(&output));
                workers.push(std::thread::spawn(move || {
                    if let Some(reply) = rpc::respond(&line, &*session) {
                        let mut output = output.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        let _ = writeln!(output, "{reply}").and_then(|()| output.flush());
                    }
                }));
            }
            // 글자로 읽을 수 없는 줄은 건너뛰고, 입력이 끝나거나 끊기면 멈춘다.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {}
            Err(_) => break,
        }
    }
    // 입력이 끝나도 이미 받은 요청의 응답은 마저 쓴다.
    for worker in workers {
        let _ = worker.join();
    }
}
