//! 원격 게이트웨이 실행 파일.
//!
//! 실행 인자:
//! - `--host <이름:포트>`: 브라우저가 쓰는 주소. 요청의 `Host`가 이 값과 같아야 한다(필수).
//! - `--login <로그인>`: 이 맥의 Tailscale 로그인. 요청의 `Tailscale-User-Login`이 이 값과 같아야 한다(필수).
//! - `--port <포트>`: 127.0.0.1에서 들을 포트. 없으면 운영체제가 빈 포트를 고르고, 고른 포트는 시작할 때 표준 출력 한 줄과 제어 소켓의 상태로 알린다.
//!
//! 같은 사용자의 게이트웨이는 하나만 뜬다(`remote.lock`). 데몬에 붙어 있는 동안만 살고, 데몬 연결이 끊기면 끝난다.
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc;

use paddock_protocol::daemon_info::{ensure_private_dir, native_dir};
use paddock_protocol::daemon_lock::DaemonLock;
use paddock_protocol::endpoint::{create_listener, remote_control_endpoint_name};
use paddock_remote::devices::Devices;
use paddock_remote::gate::GateConfig;
use paddock_remote::gateway::{Gateway, StopReason};
use paddock_remote::log::{self, Event};
use paddock_remote::{control, daemon_link, http};

/// 실행 인자.
#[derive(Debug, PartialEq, Eq)]
struct Args {
    host: String,
    login: String,
    port: u16,
}

/// 실행 인자를 읽는다. 모르는 인자·빠진 값·필수 인자 누락은 오류 문장이다.
///
/// Examples
/// --------
/// | 인자                                      | 결과                         |
/// |-------------------------------------------|------------------------------|
/// | `--host h:1 --login a@b`                  | `Ok`, 포트 0                 |
/// | `--host h:1 --login a@b --port 9000`      | `Ok`, 포트 9000              |
/// | `--host h:1`                              | `Err`(login 누락)            |
/// | `--bogus`                                 | `Err`(모르는 인자)           |
fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut host = None;
    let mut login = None;
    let mut port = 0u16;
    let mut args = args;
    let mut result = Ok(());
    while let Some(arg) = args.next() {
        if result.is_err() {
            break;
        }
        let value =
            |name: &str, args: &mut dyn Iterator<Item = String>| args.next().ok_or(format!("{name} needs a value"));
        result = match arg.as_str() {
            "--host" => value("--host", &mut args).map(|text| host = Some(text)),
            "--login" => value("--login", &mut args).map(|text| login = Some(text)),
            "--port" => value("--port", &mut args)
                .and_then(|text| text.parse::<u16>().map_err(|_| format!("--port is not a port number: {text}")))
                .map(|number| port = number),
            other => Err(format!("unknown argument: {other}")),
        };
    }
    result?;
    match (host.filter(|host| !host.is_empty()), login.filter(|login| !login.is_empty())) {
        (Some(host), Some(login)) => Ok(Args { host, login, port }),
        (None, _) => Err("--host is required".to_string()),
        (_, None) => Err("--login is required".to_string()),
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Err(message) => {
            eprintln!("paddock-remote: {message}");
            ExitCode::from(2)
        }
        Ok(args) => match run(&args) {
            Ok(reason) => {
                log::write(&Event::Stopped { reason: reason.name() });
                ExitCode::SUCCESS
            }
            Err(error) => {
                log::write(&Event::Stopped { reason: &format!("could not start: {error}") });
                eprintln!("paddock-remote: {error}");
                ExitCode::from(2)
            }
        },
    }
}

/// 게이트웨이를 띄우고 끝날 이유가 생길 때까지 기다린다. 시작에 실패하면 그 오류를 돌려준다.
fn run(args: &Args) -> Result<StopReason, Box<dyn std::error::Error>> {
    ensure_private_dir(&native_dir())?;
    let _lock = DaemonLock::try_acquire_named("remote.lock")?.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::AddrInUse, "another remote gateway is already running")
    })?;
    let link = daemon_link::connect()?;
    log::write(&Event::DaemonLinked { daemon_pid: link.info.pid, compatible: link.compatible });

    let now = Gateway::now_secs();
    let devices = Devices::open(native_dir().join("remote").join("devices.json"), now)?;
    let listener = http::bind_loopback(args.port)?;
    let port = listener.local_addr()?.port();
    let control_endpoint = remote_control_endpoint_name()?;
    let control_listener = create_listener(&control_endpoint)?;

    let (stop_tx, stop_rx) = mpsc::channel();
    let config = GateConfig { expected_host: args.host.clone(), expected_login: args.login.clone() };
    let gateway = Arc::new(Gateway::new(config, devices, port, link.info.pid, link.compatible, stop_tx.clone()));

    let http_gateway = gateway.clone();
    std::thread::spawn(move || http::serve(listener, http_gateway));
    let control_gateway = gateway.clone();
    std::thread::spawn(move || control::serve(control_listener, control_gateway));
    std::thread::spawn(move || {
        daemon_link::wait_until_closed(link.stream);
        let _ = stop_tx.send(StopReason::DaemonLost);
    });

    log::write(&Event::Started { port, host: &args.host, login: &args.login });
    let _ = writeln!(std::io::stdout(), "paddock-remote listening on 127.0.0.1:{port}");
    let reason = stop_rx.recv().unwrap_or(StopReason::DaemonLost);

    // Unix의 제어 소켓은 파일이라 직접 지운다. Windows의 파이프는 프로세스가 끝나면 사라진다.
    #[cfg(unix)]
    let _ = std::fs::remove_file(&control_endpoint);
    Ok(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn test_parse_args() {
        assert_eq!(
            parse(&["--host", "h:1", "--login", "a@b"]).unwrap(),
            Args { host: "h:1".into(), login: "a@b".into(), port: 0 }
        );
        assert_eq!(parse(&["--host", "h:1", "--login", "a@b", "--port", "9000"]).unwrap().port, 9000);
        assert!(parse(&["--host", "h:1"]).unwrap_err().contains("--login"));
        assert!(parse(&["--login", "a@b"]).unwrap_err().contains("--host"));
        assert!(parse(&["--bogus"]).unwrap_err().contains("unknown"));
        assert!(parse(&["--host"]).unwrap_err().contains("needs a value"));
        assert!(parse(&["--host", "h", "--login", "a", "--port", "70000"]).is_err());
        assert!(parse(&["--host", "", "--login", "a"]).is_err());
    }
}
