//! 데몬 접속을 공격자 입장에서 시험하는 진단 도구(배포하지 않는다).
//!
//! 사용법: `probe <connect|badtoken|dacl|fake-server|owner <pid>> [--endpoint <이름>] [--token <토큰>]`
//!
//! - `connect`: `daemon.json`의 실제 토큰으로 접속해 인사를 마친다. 다른 사용자 계정에서 실행하면 파이프를 여는 단계에서 거부돼야 한다.
//! - `badtoken`: 틀린 토큰으로 인사해, 오류 프레임을 받고 연결이 닫히는지 본다.
//! - `dacl`: 데몬 엔드포인트의 접근 권한이 소유자 전용인지 출력한다.
//! - `fake-server`: 데몬 대신 엔드포인트 이름을 선점하고, 접속한 쪽이 보내는 바이트 수를 출력한다.
//!   앱이 다른 사용자의 서버임을 알아채면 0바이트여야 한다. Windows에서는 첫 바이트를 받은 뒤 접속자의 신원을 빌려 보고(`ImpersonateNamedPipeClient`),
//!   빌린 토큰의 가장 수준을 출력한다. 접속자가 신원 확인 수준(Identification)으로만 열었다면 그 수준이어야 한다.
//! - `owner <pid>`: 그 프로세스를 실행한 사용자가 이 사용자와 같은지 앱이 서버를 확인할 때와 같은 운영체제 조회로 출력한다.
//!   다른 사용자의 프로세스(예: 시스템 서비스)를 대상으로 하면 앱이 그 서버를 거부하는 판정(OTHER-USER 또는 CANNOT-VERIFY)을 실제 운영체제 값으로 보인다.
//!
//! - `--endpoint`: 접속하거나 선점할 엔드포인트 이름. 없으면 `connect`·`badtoken`·`dacl`은 `daemon.json`의 값을, `fake-server`는 이 사용자의 기본 이름을 쓴다.
//!   `daemon.json`이 없어도 이 값과 `--token`만 있으면 `connect`·`badtoken`이 돈다.
//! - `--verified`: `connect`에서 앱과 같은 `connect_verified`로 접속한다. 서버 프로세스의 소유자가 이 사용자가 아니면 인사(hello)를 보내기 전에 거부한다.
//!   다른 사용자(시스템 서비스)가 연 파이프를 `--endpoint`로 주면 가짜 서버 선점을 실제 운영체제 값으로 재연한다. 이름이 맞아도 토큰은 보내지 않는다.
//! - `--plain`: `connect`에서 Paddock의 `endpoint::connect` 대신 라이브러리 기본 설정으로 접속한다. 가장 수준 측정의 대조군이다.
//! - `--token`: 인사에 쓸 토큰. 없으면 `connect`는 `daemon.json`의 값을, `badtoken`은 틀린 값을 쓴다.
use std::io::{self, Read};

use interprocess::local_socket::traits::{ListenerExt as _, Stream as _};
use interprocess::local_socket::{GenericFilePath, Stream, ToFsName as _};
use paddock_protocol::daemon_info::read_info;
use paddock_protocol::endpoint::{connect, connect_verified, create_listener, endpoint_name};
use paddock_protocol::frame::Frame;
use paddock_protocol::message::{Hello, HelloOk, PROTOCOL_VERSION, kind};

/// 명령줄에서 읽은 선택 인자.
struct Options {
    endpoint: Option<String>,
    token: Option<String>,
    plain: bool,
    verified: bool,
}

/// `--name value` 형태의 인자 값을 찾는다.
fn option_value(
    args: &[String],
    name: &str,
) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options {
        endpoint: option_value(&args, "--endpoint"),
        token: option_value(&args, "--token"),
        plain: args.iter().any(|arg| arg == "--plain"),
        verified: args.iter().any(|arg| arg == "--verified"),
    };
    let outcome = match args.first().map(String::as_str) {
        Some("connect") => connect_with_real_token(&options),
        Some("badtoken") => connect_with_wrong_token(&options),
        Some("dacl") => show_endpoint_permissions(),
        Some("fake-server") => run_fake_server(&options),
        Some("owner") => show_process_owner(args.get(1)),
        _ => Err(io::Error::other(
            "usage: probe <connect|badtoken|dacl|fake-server|owner <pid>> [--endpoint <name>] [--token <token>]",
        )),
    };
    if let Err(error) = outcome {
        println!("FAILED {error} (os error {:?})", error.raw_os_error());
        std::process::exit(1);
    }
}

fn daemon_info() -> io::Result<paddock_protocol::daemon_info::DaemonInfo> {
    read_info().ok_or_else(|| io::Error::other("daemon.json is missing or unreadable"))
}

/// 접속할 엔드포인트와 인사에 쓸 토큰. 지정하지 않은 값만 `daemon.json`에서 읽는다.
fn target_of(
    options: &Options,
    default_token: Option<&str>,
) -> io::Result<(String, String)> {
    let endpoint = match &options.endpoint {
        Some(endpoint) => endpoint.clone(),
        None => daemon_info()?.endpoint,
    };
    let token = match (&options.token, default_token) {
        (Some(token), _) => token.clone(),
        (None, Some(token)) => token.to_string(),
        (None, None) => daemon_info()?.token,
    };
    Ok((endpoint, token))
}

fn connect_with_real_token(options: &Options) -> io::Result<()> {
    let (endpoint, token) = target_of(options, None)?;
    let stream = if options.plain {
        Some(Stream::connect(endpoint.to_fs_name::<GenericFilePath>()?.into_owned())?)
    } else if options.verified {
        // 서버가 다른 사용자의 프로세스면 인사를 쓰기 전에 오류로 끝난다.
        match connect_verified(&endpoint) {
            Ok(stream) => Some(stream),
            Err(error) => {
                println!("REFUSED before any byte was sent: {error}");
                None
            }
        }
    } else {
        Some(connect(&endpoint)?)
    };
    if let Some(stream) = stream {
        let (mut recv, mut send) = stream.split();
        Frame::json(kind::HELLO, &Hello { token, protocol_version: PROTOCOL_VERSION }).write_to(&mut send)?;
        let reply = Frame::read_from(&mut recv)?;
        if reply.kind == kind::HELLO_OK {
            let ok: HelloOk = reply.parse_json()?;
            println!("CONNECTED daemon pid {} fingerprint {}", ok.pid, ok.fingerprint);
        } else {
            println!("REJECTED kind {} {}", reply.kind, reply.as_text());
        }
    }
    Ok(())
}

fn connect_with_wrong_token(options: &Options) -> io::Result<()> {
    let (endpoint, token) = target_of(options, Some("wrong"))?;
    let stream = connect(&endpoint)?;
    let (mut recv, mut send) = stream.split();
    Frame::json(kind::HELLO, &Hello { token, protocol_version: PROTOCOL_VERSION }).write_to(&mut send)?;
    let reply = Frame::read_from(&mut recv)?;
    match reply.kind {
        kind::ERR => println!("bad token rejected: {}", reply.as_text()),
        other => println!("UNEXPECTED: kind {other} for a wrong token"),
    }
    match Frame::read_from(&mut recv) {
        Ok(extra) => println!("UNEXPECTED: connection stayed open (kind {})", extra.kind),
        Err(error) => println!("connection closed after the error ({error})"),
    }
    Ok(())
}

#[cfg(windows)]
fn show_endpoint_permissions() -> io::Result<()> {
    use paddock_protocol::windows_security::{current_user_sid, object_sddl};
    let info = daemon_info()?;
    let sid = current_user_sid()?;
    let sddl = object_sddl(&info.endpoint)?;
    // 소유자는 나, DACL은 보호됨(P)에 나만 모든 권한을 가진 항목 하나다. GA는 파일 개체 기준으로 FA로 풀려 보인다.
    let owner_only = sddl.starts_with(&format!("O:{sid}"))
        && sddl.contains("D:P(A;;")
        && sddl.contains(&format!(";;;{sid})"))
        && sddl.matches("(A;").count() == 1
        && !sddl.contains("(D;")
        && sddl.matches('(').count() == 1;
    println!("pipe {} SDDL {sddl}", info.endpoint);
    println!("current user SID {sid}");
    println!("VERDICT {}", if owner_only { "OWNER-ONLY (one entry, protected DACL)" } else { "NOT owner-only" });
    Ok(())
}

#[cfg(unix)]
fn show_endpoint_permissions() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let info = daemon_info()?;
    let socket_mode = std::fs::metadata(&info.endpoint)?.permissions().mode() & 0o7777;
    let parent = std::path::Path::new(&info.endpoint).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let dir_mode = std::fs::metadata(&parent)?.permissions().mode() & 0o7777;
    println!("socket {} mode {socket_mode:o}, folder {} mode {dir_mode:o}", info.endpoint, parent.display());
    println!("VERDICT {}", if socket_mode == 0o600 && dir_mode == 0o700 { "OWNER-ONLY" } else { "NOT owner-only" });
    Ok(())
}

/// 프로세스 소유자가 이 사용자와 같은지 보인다. 앱의 서버 확인(`peer_user`)이 쓰는 조회와 같고, 같다고 확인된 때만 앱이 연결을 이어 간다.
fn show_process_owner(pid_text: Option<&String>) -> io::Result<()> {
    let pid: u32 =
        pid_text.and_then(|text| text.parse().ok()).ok_or_else(|| io::Error::other("usage: probe owner <pid>"))?;
    #[cfg(windows)]
    let (own, owner) = (
        paddock_protocol::windows_security::current_user_sid()?,
        paddock_protocol::windows_security::process_user_sid(pid),
    );
    #[cfg(unix)]
    let (own, owner) = {
        use std::os::unix::fs::MetadataExt;
        (
            format!("uid:{}", unsafe { libc::geteuid() }),
            std::fs::metadata(format!("/proc/{pid}")).map(|meta| format!("uid:{}", meta.uid())),
        )
    };
    println!("own user {own}");
    match owner {
        Ok(owner) if owner == own => println!("process {pid} user {owner}\nVERDICT SAME-USER (the app would continue)"),
        Ok(owner) => println!(
            "process {pid} user {owner}\nVERDICT OTHER-USER (the app would refuse: Daemon is owned by another user)"
        ),
        Err(error) => println!(
            "process {pid} user unreadable: {error}\nVERDICT CANNOT-VERIFY (the app would refuse: Could not verify the other side of the connection)"
        ),
    }
    Ok(())
}

fn run_fake_server(options: &Options) -> io::Result<()> {
    let endpoint = match &options.endpoint {
        Some(endpoint) => endpoint.clone(),
        None => endpoint_name()?,
    };
    let listener = create_listener(&endpoint)?;
    println!("fake server listening on {endpoint}");
    for stream in listener.incoming().flatten() {
        // 쪼개기 전에 서버 쪽 파이프 핸들을 잡아 둔다. 쪼갠 뒤에도 같은 핸들이 살아 있다.
        #[cfg(windows)]
        let pipe = {
            use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
            match &stream {
                Stream::NamedPipe(named) => named.as_handle().as_raw_handle(),
            }
        };
        let (mut recv, _send) = stream.split();
        #[cfg(windows)]
        println!("impersonation before any byte arrived: {}", impersonation_outcome(pipe));
        let mut n_bytes = 0usize;
        let mut buf = [0u8; 4096];
        // 접속한 쪽이 끊을 때까지 받은 바이트를 센다.
        while let Ok(n) = recv.read(&mut buf) {
            if n == 0 {
                break;
            }
            #[cfg(windows)]
            if n_bytes == 0 {
                println!("impersonation after the first {n} bytes: {}", impersonation_outcome(pipe));
            }
            n_bytes += n;
        }
        println!("connection closed after {n_bytes} bytes received");
    }
    Ok(())
}

/// 파이프 서버가 접속자의 신원을 빌려 보고 결과를 글로 돌려준다: 실패하면 오류 번호, 성공하면 빌린 토큰의 가장 수준.
#[cfg(windows)]
fn impersonation_outcome(pipe: std::os::windows::io::RawHandle) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, RevertToSelf, TOKEN_QUERY, TokenImpersonationLevel};
    use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

    /// 가장 수준 번호의 이름. `SECURITY_IMPERSONATION_LEVEL` 순서다.
    fn level_name(level: i32) -> &'static str {
        match level {
            0 => "Anonymous",
            1 => "Identification",
            2 => "Impersonation",
            3 => "Delegation",
            _ => "unknown",
        }
    }

    unsafe {
        if ImpersonateNamedPipeClient(pipe.cast()) == 0 {
            let error = io::Error::last_os_error();
            format!("FAILED os error {:?} ({error})", error.raw_os_error())
        } else {
            let mut token = std::ptr::null_mut();
            let opened = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token);
            let outcome = if opened == 0 {
                format!("impersonated, but the thread token cannot be opened: {}", io::Error::last_os_error())
            } else {
                let mut level = 0i32;
                let mut n_bytes = 0u32;
                let read = GetTokenInformation(
                    token,
                    TokenImpersonationLevel,
                    (&mut level as *mut i32).cast(),
                    std::mem::size_of::<i32>() as u32,
                    &mut n_bytes,
                );
                CloseHandle(token);
                if read == 0 {
                    format!("impersonated, but the level cannot be read: {}", io::Error::last_os_error())
                } else {
                    format!("SUCCEEDED level {level} ({})", level_name(level))
                }
            };
            RevertToSelf();
            outcome
        }
    }
}
