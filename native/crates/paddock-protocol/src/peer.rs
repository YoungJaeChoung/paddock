//! 연결 상대 프로세스의 사용자 확인. 데몬은 붙는 앱이, 앱은 붙은 데몬이 자기와 같은 사용자인지 본다.
use std::fmt;
use std::io;

use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::StreamCommon;

/// 운영체제가 사용자에게 붙이는 식별값(Unix는 uid, Windows는 SID). 같은지만 비교한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserId(String);

impl UserId {
    /// 식별값 문자열로 사용자를 만든다. 운영체제에 묻지 않으므로, 상대가 다른 사용자인 경우의 거부 규칙을 값으로 확인할 때 쓴다(데몬 크레이트의 시험도 쓰므로 공개한다).
    pub fn from_raw(raw: &str) -> Self {
        Self(raw.to_string())
    }
}

/// 확인하는 상대가 연결의 어느 쪽인가.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerRole {
    /// 데몬이 앱을 확인한다.
    Client,
    /// 앱이 데몬을 확인한다.
    Server,
}

/// 상대 확인이 실패한 이유.
#[derive(Debug)]
pub enum PeerError {
    /// 상대의 사용자를 알아내지 못했다. 확인할 수 없으면 연결하지 않는다.
    Io(io::Error),
    /// 상대가 다른 사용자다.
    OtherUser(PeerRole),
}

impl fmt::Display for PeerError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Could not verify the other side of the connection: {error}"),
            Self::OtherUser(PeerRole::Server) => f.write_str("Daemon is owned by another user"),
            Self::OtherUser(PeerRole::Client) => f.write_str("Connection from another user was refused"),
        }
    }
}

impl std::error::Error for PeerError {}

impl From<io::Error> for PeerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// 연결 상대의 사용자를 운영체제에 물을 수 있는 연결.
pub trait PeerCredentials {
    /// 연결 상대 프로세스를 실행한 사용자. 운영체제에 물을 수 없으면 오류다.
    fn peer_user(&self) -> io::Result<UserId>;
}

/// 이 프로세스를 실행한 사용자.
pub fn current_user() -> io::Result<UserId> {
    #[cfg(unix)]
    let user = Ok(UserId(format!("uid:{}", unsafe { libc::geteuid() })));
    #[cfg(windows)]
    let user = crate::windows_security::current_user_sid().map(UserId);
    user
}

/// 두 사용자 식별값이 같은 사용자를 가리키는가.
pub fn is_same_user(
    peer: &UserId,
    own: &UserId,
) -> bool {
    peer == own
}

/// 상대가 `own`과 같은 사용자일 때만 연결을 돌려준다. 다르면 연결을 닫고(버리고) 오류다.
///
/// 호출자는 이 검사를 지난 연결만 받으므로, 다른 사용자의 가짜 서버에는 인사(hello)를 포함해 한 바이트도 보내지 못한다.
pub fn require_same_user<S: PeerCredentials>(
    stream: S,
    role: PeerRole,
    own: &UserId,
) -> Result<S, PeerError> {
    let peer = stream.peer_user()?;
    if is_same_user(&peer, own) { Ok(stream) } else { Err(PeerError::OtherUser(role)) }
}

impl PeerCredentials for Stream {
    fn peer_user(&self) -> io::Result<UserId> {
        let creds = self.peer_creds()?;
        #[cfg(unix)]
        let user = creds
            .euid()
            .map(|uid| UserId(format!("uid:{uid}")))
            .ok_or_else(|| io::Error::other("peer user id is not available"));
        #[cfg(windows)]
        let user = match creds.pid() {
            Some(pid) => crate::windows_security::process_user_sid(pid).map(UserId),
            None => Err(io::Error::other("peer process id is not available")),
        };
        user
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Read, Write};
    use std::sync::{Arc, Mutex};

    /// 상대 사용자가 고정돼 있고 쓴 내용을 기록하는 가짜 연결.
    struct FakeStream {
        peer: UserId,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl PeerCredentials for FakeStream {
        fn peer_user(&self) -> io::Result<UserId> {
            Ok(self.peer.clone())
        }
    }

    impl Write for FakeStream {
        fn write(
            &mut self,
            buf: &[u8],
        ) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for FakeStream {
        fn read(
            &mut self,
            _buf: &mut [u8],
        ) -> io::Result<usize> {
            Ok(0)
        }
    }

    fn user(name: &str) -> UserId {
        UserId::from_raw(name)
    }

    #[test]
    fn test_c_t1_peer_user_mismatch_rejected() {
        let own = user("1000");
        assert!(is_same_user(&user("1000"), &own));
        assert!(!is_same_user(&user("1001"), &own));
        let written = Arc::new(Mutex::new(Vec::new()));
        let stream = FakeStream { peer: user("1001"), written };
        let error = require_same_user(stream, PeerRole::Client, &own).err().expect("other user must be rejected");
        assert!(matches!(error, PeerError::OtherUser(PeerRole::Client)));
    }

    #[test]
    fn test_c_t2_no_frames_sent_when_server_owner_differs() {
        let own = user("1000");
        let written = Arc::new(Mutex::new(Vec::new()));
        let stream = FakeStream { peer: user("1001"), written: written.clone() };
        // 호출자는 이 검사를 통과한 연결만 받으므로, 통과 전에는 인사(hello)를 포함해 아무것도 쓸 수 없다.
        let error =
            require_same_user(stream, PeerRole::Server, &own).err().expect("other user's server must be rejected");
        assert_eq!(error.to_string(), "Daemon is owned by another user");
        assert!(written.lock().unwrap().is_empty());
    }

    #[test]
    fn test_same_user_passes_the_stream_through() {
        let own = user("1000");
        let written = Arc::new(Mutex::new(Vec::new()));
        let stream = FakeStream { peer: user("1000"), written: written.clone() };
        let mut stream = require_same_user(stream, PeerRole::Server, &own).expect("same user");
        stream.write_all(b"x").unwrap();
        assert_eq!(*written.lock().unwrap(), b"x");
    }

    #[test]
    fn test_peer_error_messages_are_sentences() {
        assert_eq!(PeerError::OtherUser(PeerRole::Client).to_string(), "Connection from another user was refused");
        let io_error = PeerError::Io(io::Error::other("boom"));
        assert!(io_error.to_string().contains("boom"));
    }
}
