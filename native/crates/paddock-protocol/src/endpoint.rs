//! 데몬 엔드포인트: 이름 정하기, 리스너 만들기(소유자 전용), 접속.
//!
//! Windows는 명명된 파이프(`\\.\pipe\paddock-<사용자 SID 해시>`), Unix는 소켓 파일이다. TCP 포트는 열지 않는다.
use std::io;

#[cfg(unix)]
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericFilePath, Listener, ListenerOptions, Stream, ToFsName};

use crate::peer::{PeerError, PeerRole, current_user, require_same_user};

/// Unix 소켓 경로의 최대 길이. `sun_path`는 Linux 108·macOS 104바이트(끝의 NUL 포함)라 여유를 두고 맞춘다.
#[cfg(unix)]
const SOCKET_PATH_MAX: usize = 100;

/// 이 사용자의 데몬 엔드포인트 이름. 같은 사용자에게는 항상 같은 이름이다.
///
/// Windows는 사용자 SID의 해시가 들어가 다른 계정이 같은 이름을 추측해 선점하기 어렵다.
/// Unix는 `$XDG_RUNTIME_DIR/paddock/daemon.sock`, 그 폴더가 없으면 `~/.paddock/native/run/daemon.sock`이다.
/// 이 경로가 소켓 주소 길이 한도(`SOCKET_PATH_MAX`)를 넘으면 `/tmp/paddock-<uid>/daemon.sock`을 쓰고, 그것도 넘으면 오류다.
pub fn endpoint_name() -> io::Result<String> {
    #[cfg(windows)]
    let name = {
        use sha2::{Digest, Sha256};
        let sid = crate::windows_security::current_user_sid()?;
        let hash: String = Sha256::digest(sid.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect();
        Ok(format!(r"\\.\pipe\paddock-{hash}"))
    };
    #[cfg(unix)]
    let name = {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).filter(|dir| dir.is_dir());
        let fallback_dir = std::path::PathBuf::from(format!("/tmp/paddock-{}", unsafe { libc::geteuid() }));
        socket_path(runtime_dir.as_deref(), &crate::daemon_info::native_dir(), &fallback_dir)
    };
    name
}

/// Unix 소켓 파일 경로를 정한다. 파일 시스템도 환경 변수도 건드리지 않는 계산이다.
///
/// 런타임 폴더(`<runtime_dir>/paddock`)가 있으면 그 아래, 없으면 `<native_dir>/run` 아래의 `daemon.sock`이 우선이다.
/// 그 경로가 `SOCKET_PATH_MAX`를 넘으면 `<fallback_dir>/daemon.sock`을 쓰고, 그것도 넘으면 오류다.
///
/// Examples
/// --------
/// | runtime_dir | native_dir 길이 | 결과                              |
/// |-------------|-----------------|-----------------------------------|
/// | `/run/u/1`  | (무관)          | `/run/u/1/paddock/daemon.sock`    |
/// | 없음        | 짧음            | `<native_dir>/run/daemon.sock`    |
/// | 없음        | 한도 초과       | `<fallback_dir>/daemon.sock`      |
#[cfg(unix)]
fn socket_path(
    runtime_dir: Option<&std::path::Path>,
    native_dir: &std::path::Path,
    fallback_dir: &std::path::Path,
) -> io::Result<String> {
    let dir = match runtime_dir {
        Some(dir) => dir.join("paddock"),
        None => native_dir.join("run"),
    };
    let primary = dir.join("daemon.sock").to_string_lossy().into_owned();
    let fallback = fallback_dir.join("daemon.sock").to_string_lossy().into_owned();
    if primary.len() <= SOCKET_PATH_MAX {
        Ok(primary)
    } else if fallback.len() <= SOCKET_PATH_MAX {
        Ok(fallback)
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "socket path is too long"))
    }
}

/// 현재 사용자만 접속할 수 있는 리스너를 만든다.
///
/// Windows는 소유자 SID 한 항목만 둔 보호된 DACL로 파이프를 만들고, 같은 이름의 파이프가 이미 있으면 실패한다(데몬 하나만 허용).
/// Unix는 폴더 0700·소켓 0600이고, 이전 데몬이 남긴 소켓 파일은 덮어쓴다. 소켓 폴더는 현재 사용자 소유의 실제 폴더(심볼릭 링크 아님)여야 한다.
/// 덮어쓰기는 살아 있는 데몬의 소켓도 빼앗으므로, 호출자는 `DaemonLock`을 쥔 채로 불러야 한다.
pub fn create_listener(endpoint: &str) -> io::Result<Listener> {
    let name = endpoint.to_fs_name::<GenericFilePath>()?.into_owned();
    let options = ListenerOptions::new().name(name);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::local_socket::ListenerOptionsExt;
        use interprocess::os::windows::security_descriptor::SecurityDescriptor;
        let sddl = pipe_sddl()?;
        let wide = widestring::U16CString::from_str(&sddl).map_err(io::Error::other)?;
        options.security_descriptor(SecurityDescriptor::deserialize(&wide)?)
    };
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        if let Some(dir) = std::path::Path::new(endpoint).parent() {
            verify_own_folder(dir)?;
            crate::daemon_info::ensure_private_dir(dir)?;
        }
        options.mode(0o600).try_overwrite(true)
    };
    options.create_sync()
}

/// 데몬 파이프를 만들 때 거는 접근 권한(SDDL). 보호된(P) DACL에 현재 사용자 SID 하나만 모든 권한을 준다.
/// SYSTEM·Administrators·다른 계정은 없다.
#[cfg(windows)]
pub fn pipe_sddl() -> io::Result<String> {
    Ok(format!("D:P(A;;GA;;;{})", crate::windows_security::current_user_sid()?))
}

/// 폴더가 있으면 심볼릭 링크가 아닌 현재 사용자 소유의 폴더인지 확인한다. 없으면 통과한다(곧 만든다).
///
/// `/tmp` 아래처럼 다른 사용자가 미리 만들어 둘 수 있는 위치의 소켓 폴더를 믿지 않기 위한 검사다.
#[cfg(unix)]
fn verify_own_folder(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Socket folder is not a real folder owned by the current user",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// 엔드포인트에 접속한다. 상대가 누구인지는 확인하지 않는다.
///
/// Windows는 서버가 접속한 쪽의 신원만 알 수 있도록(`SECURITY_IDENTIFICATION`) 파이프를 연다.
/// 기본값으로 열면 가짜 서버가 `ImpersonateNamedPipeClient`로 앱 사용자의 권한을 빌려 쓸 수 있다.
pub fn connect(endpoint: &str) -> io::Result<Stream> {
    #[cfg(unix)]
    let stream = {
        let name = endpoint.to_fs_name::<GenericFilePath>()?.into_owned();
        Stream::connect(name)
    };
    #[cfg(windows)]
    let stream = connect_identification_only(endpoint);
    stream
}

/// 가장(impersonation)을 막는 서비스 품질 플래그로 파이프를 열어 `Stream`으로 감싼다.
#[cfg(windows)]
fn connect_identification_only(endpoint: &str) -> io::Result<Stream> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use std::time::{Duration, Instant};

    use interprocess::os::windows::named_pipe::local_socket::Stream as PipeStream;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    };

    /// 모든 파이프 인스턴스가 사용 중일 때의 오류 번호.
    const ERROR_PIPE_BUSY: i32 = 231;
    /// 사용 중인 파이프를 기다리는 최대 시간과 재시도 간격.
    const BUSY_WAIT: Duration = Duration::from_secs(2);
    const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(20);

    let wide = widestring::U16CString::from_str(endpoint).map_err(io::Error::other)?;
    let end = Instant::now() + BUSY_WAIT;
    let handle = loop {
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if raw != INVALID_HANDLE_VALUE {
            break Ok(unsafe { OwnedHandle::from_raw_handle(raw as _) });
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_BUSY) || Instant::now() >= end {
            break Err(error);
        }
        std::thread::sleep(BUSY_RETRY_INTERVAL);
    }?;
    PipeStream::try_from(handle).map(Stream::from).map_err(|error| io::Error::other(error.to_string()))
}

/// 접속한 서버가 현재 사용자의 프로세스일 때만 연결을 돌려준다.
///
/// 다른 사용자가 같은 이름을 선점한 가짜 데몬이면 아무것도 보내지 않고 연결을 닫는다.
pub fn connect_verified(endpoint: &str) -> Result<Stream, PeerError> {
    let stream = connect(endpoint)?;
    require_same_user(stream, PeerRole::Server, &current_user()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::peer::{PeerCredentials, current_user};
    use crate::test_home::TempHome;

    #[test]
    fn test_endpoint_name_is_stable() {
        let _home = TempHome::new();
        let first = endpoint_name().unwrap();
        assert_eq!(first, endpoint_name().unwrap());
        #[cfg(windows)]
        assert!(first.starts_with(r"\\.\pipe\paddock-"), "{first}");
        #[cfg(unix)]
        assert!(first.ends_with("/.paddock/native/run/daemon.sock"), "{first}");
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_endpoint_prefers_runtime_dir() {
        let mut home = TempHome::new();
        let runtime = home.dir.path().join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        home.set("XDG_RUNTIME_DIR", Some(runtime.as_os_str()));
        assert_eq!(endpoint_name().unwrap(), runtime.join("paddock").join("daemon.sock").to_string_lossy());
    }

    #[test]
    fn test_listener_accepts_and_peers_are_the_current_user() {
        let _home = TempHome::new();
        let name = endpoint_name().unwrap();
        let listener = create_listener(&name).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = interprocess::local_socket::traits::Listener::accept(&listener).unwrap();
            let own = current_user().unwrap();
            assert_eq!(stream.peer_user().unwrap(), own);
            let frame = Frame::read_from(&mut stream).unwrap();
            frame.write_to(&mut stream).unwrap();
        });
        let mut client = connect(&name).unwrap();
        assert_eq!(client.peer_user().unwrap(), current_user().unwrap());
        Frame::new(4, b"ping".to_vec()).write_to(&mut client).unwrap();
        let echoed = Frame::read_from(&mut client).unwrap();
        assert_eq!(echoed.payload, b"ping");
        server.join().unwrap();
    }

    #[test]
    fn test_connect_verified_accepts_a_server_of_the_same_user() {
        let _home = TempHome::new();
        let name = endpoint_name().unwrap();
        let listener = create_listener(&name).unwrap();
        let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = interprocess::local_socket::traits::Listener::accept(&listener).unwrap();
            accepted_tx.send(()).unwrap();
            // 클라이언트가 끊을 때까지 연결을 쥔다.
            let mut buf = [0u8; 1];
            let _ = std::io::Read::read(&mut stream, &mut buf);
        });
        let client = connect_verified(&name);
        assert!(client.is_ok());
        accepted_rx.recv().unwrap();
        drop(client);
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_socket_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let _home = TempHome::new();
        let name = endpoint_name().unwrap();
        let _listener = create_listener(&name).unwrap();
        let path = std::path::Path::new(&name);
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn test_stale_socket_file_is_replaced() {
        let _home = TempHome::new();
        let name = endpoint_name().unwrap();
        // 데몬이 죽은 뒤처럼 소켓 파일만 남긴다. 표준 라이브러리의 소켓은 닫혀도 파일을 지우지 않는다.
        crate::daemon_info::ensure_private_dir(std::path::Path::new(&name).parent().unwrap()).unwrap();
        drop(std::os::unix::net::UnixListener::bind(&name).unwrap());
        assert!(std::path::Path::new(&name).exists(), "the stale socket file must remain");
        assert!(connect(&name).is_err(), "nobody answers on a stale socket");
        // 새 데몬은 남은 파일을 덮어쓰고 리스너를 만들 수 있다.
        let again = create_listener(&name).unwrap();
        let server = std::thread::spawn(move || {
            drop(interprocess::local_socket::traits::Listener::accept(&again).unwrap());
        });
        assert!(connect(&name).is_ok());
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_long_native_dir_falls_back_to_a_short_socket_path() {
        let home = TempHome::new();
        let long_native = home.dir.path().join("h".repeat(100)).join(".paddock").join("native");
        // 실제 /tmp 폴더가 아니라 임시 폴더 아래를 대체 위치로 쓴다.
        let fallback_dir = home.dir.path().join("short");
        let name = socket_path(None, &long_native, &fallback_dir).unwrap();
        // sun_path 한도(Linux 108, macOS 104) 안이어야 접속할 수 있다.
        assert!(name.len() <= SOCKET_PATH_MAX, "{name}");
        assert_eq!(name, fallback_dir.join("daemon.sock").to_string_lossy());
        // 대체 위치에서 실제로 리스너를 만들 수 있다.
        let listener = create_listener(&name).unwrap();
        drop(listener);
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_path_rules() {
        use std::path::Path;
        let short_native = Path::new("/home/a/.paddock/native");
        let fallback = Path::new("/tmp/paddock-1000");
        assert_eq!(
            socket_path(Some(Path::new("/run/user/1000")), short_native, fallback).unwrap(),
            "/run/user/1000/paddock/daemon.sock"
        );
        assert_eq!(socket_path(None, short_native, fallback).unwrap(), "/home/a/.paddock/native/run/daemon.sock");
        let long_native = Path::new("/home").join("h".repeat(120));
        assert_eq!(socket_path(None, &long_native, fallback).unwrap(), "/tmp/paddock-1000/daemon.sock");
        let error = socket_path(None, &long_native, &Path::new("/tmp").join("p".repeat(120))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn test_endpoint_name_falls_back_under_tmp_for_the_current_user() {
        let mut home = TempHome::new();
        let long_home = home.dir.path().join("h".repeat(100));
        std::fs::create_dir_all(&long_home).unwrap();
        home.set("HOME", Some(long_home.as_os_str()));
        // 이름만 계산한다. 이 경로에 파일이나 폴더를 만들지 않는다.
        let name = endpoint_name().unwrap();
        assert_eq!(name, format!("/tmp/paddock-{}/daemon.sock", unsafe { libc::geteuid() }));
    }

    #[cfg(unix)]
    #[test]
    fn test_socket_folder_must_be_a_real_folder_owned_by_the_user() {
        let home = TempHome::new();
        let real = home.dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = home.dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(verify_own_folder(&real).is_ok());
        assert!(verify_own_folder(&home.dir.path().join("missing")).is_ok(), "a missing folder is created later");
        let error = verify_own_folder(&link).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_pipe_dacl_has_only_the_current_user() {
        let _home = TempHome::new();
        let name = endpoint_name().unwrap();
        let _listener = create_listener(&name).unwrap();
        let sid = crate::windows_security::current_user_sid().unwrap();
        let sddl = crate::windows_security::object_sddl(&name).unwrap();
        assert!(sddl.contains("D:P"), "{sddl}");
        assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
        assert!(sddl.contains(&sid), "{sddl}");
    }

    /// 서버가 접속자의 신원을 빌려 본 토큰의 가장 수준 번호(1 = Identification, 2 = Impersonation). 빌리지 못하면 오류 번호.
    #[cfg(windows)]
    fn impersonation_level_seen_by_server(pipe: std::os::windows::io::RawHandle) -> Result<i32, i32> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Security::{GetTokenInformation, RevertToSelf, TOKEN_QUERY, TokenImpersonationLevel};
        use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
        use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
        unsafe {
            if ImpersonateNamedPipeClient(pipe.cast()) == 0 {
                Err(io::Error::last_os_error().raw_os_error().unwrap_or(-1))
            } else {
                let mut token = std::ptr::null_mut();
                let mut level = 0i32;
                let mut n_bytes = 0u32;
                let read = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) != 0
                    && GetTokenInformation(
                        token,
                        TokenImpersonationLevel,
                        (&mut level as *mut i32).cast(),
                        std::mem::size_of::<i32>() as u32,
                        &mut n_bytes,
                    ) != 0;
                if !token.is_null() {
                    CloseHandle(token);
                }
                RevertToSelf();
                if read { Ok(level) } else { Err(-1) }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_connect_lets_the_server_see_identification_only() {
        use std::io::Write as _;
        use std::os::windows::io::{AsHandle as _, AsRawHandle as _};
        // 사용자의 실제 데몬 파이프와 겹치지 않도록 이 시험 전용 이름을 쓴다.
        let name = format!(r"\\.\pipe\paddock-test-identification-{}", std::process::id());
        let listener = create_listener(&name).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = interprocess::local_socket::traits::Listener::accept(&listener).unwrap();
            let mut byte = [0u8; 1];
            std::io::Read::read_exact(&mut stream, &mut byte).unwrap();
            let pipe = match &stream {
                Stream::NamedPipe(named) => named.as_handle().as_raw_handle(),
            };
            impersonation_level_seen_by_server(pipe)
        });
        let mut client = connect(&name).unwrap();
        client.write_all(b"x").unwrap();
        // 기본 설정으로 열면 2(Impersonation)가 보인다. 1(Identification)이면 서버는 신원만 알 뿐 권한을 빌려 쓸 수 없다.
        assert_eq!(server.join().unwrap(), Ok(1));
    }
}
