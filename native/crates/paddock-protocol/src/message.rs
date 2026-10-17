//! 앱↔데몬 메시지: 프레임 종류 번호와 JSON 내용의 구조체.
/// 셸 대신 직접 띄우는 프로그램의 실행 명세. 규칙은 core의 `account_launch`가 쥔다.
pub use paddock_core::account_launch::Launch;
use paddock_core::agent_account::{AccountProfile, Scope};
use paddock_core::agent_model::ActivityState;
use serde::{Deserialize, Serialize};

/// 앱과 데몬이 맞춰야 하는 프로토콜 버전. 메시지 모양이 바뀌면 올린다.
pub const PROTOCOL_VERSION: u32 = 1;

/// 프레임 종류 번호. 1~99는 앱 → 데몬, 100 이상은 데몬 → 앱이다.
///
/// 연결은 한 번에 터미널 하나에 묶인다.
///
/// ```text
/// 연결됨 ── HELLO(맞음) ──► 인사 마침(HELLO_OK) ── NEW 또는 ATTACH 성공(ATTACHED) ──► 터미널에 묶임
///   │ HELLO 틀림 → ERR 뒤 끊김                         │ INPUT·RESIZE는 묶인 터미널에만 적용
///   │ HELLO 아님 → 답 없이 끊김                        │ 묶인 터미널의 REPLAY → REPLAY_END → OUTPUT… → EXITED
///                                                      │ NEW·ATTACH를 다시 보내면 새 터미널로 옮겨 묶임
///                                                      │ 읽는 속도가 느리면 RESYNC → REPLAY… → REPLAY_END 로 화면 전체를 다시 받음
///                                                      │ 다른 연결이 ATTACH하면 DETACHED 를 받고 출력이 끊김
///                                                      │ 묶인 터미널의 상태(폴더·프로그램·활동)가 바뀔 때마다 TERMINAL_STATE
/// ```
///
/// INPUT·RESIZE·OUTPUT에 터미널 번호가 없는 것은 이 묶임 때문이다. 앱은 터미널마다 연결을 하나씩 연다.
///
/// 터미널의 출력을 받는 연결은 한 번에 하나다. 이미 다른 연결이 받는 터미널에 ATTACH하면 새 연결이 출력을 이어받고,
/// 밀려난 연결은 `DETACHED`를 받는다. 밀려난 연결도 연결이 끊기기 전까지는 INPUT·RESIZE를 보낼 수 있고, 그 입력은 셸에 그대로 들어간다.
///
/// 연결마다 보내지 못한 출력을 데몬 메모리에 쌓는 양에는 상한이 있다. 클라이언트가 소켓을 읽지 못해 상한을 넘으면
/// 데몬은 그 연결에 OUTPUT을 보내지 않고 버린다(링에는 계속 쌓인다). 클라이언트가 다시 읽어 밀린 프레임이 비면
/// 데몬이 `RESYNC` → `REPLAY`(0개 이상) → `REPLAY_END`로 링 전체를 다시 보낸다. 클라이언트는 `RESYNC`를 받으면
/// 지금까지 쌓은 화면을 버리고 이어지는 `REPLAY`로 화면을 처음부터 다시 만들어야 한다.
/// 요청마다 답이 있다: NEW·ATTACH는 `ATTACHED`, LIST·KILL은 `LIST_REPLY`, STATUS_REQUEST·SHUTDOWN은 `STATUS`,
/// 실패는 모두 `ERR`다. INPUT·RESIZE·BYE는 답이 없다.
///
/// `TERMINAL_STATE`는 요청 없이 데몬이 보내는 알림이다. 값이 바뀔 때만, 그리고 묶이는 순간 현재 값을 한 번 보낸다.
/// `finished`는 완료를 알리는 그 프레임에서만 참이다. 앱은 모르는 종류의 프레임을 버려야 하므로(새 알림을 더해도 옛 앱이 깨지지 않게)
/// 프로토콜 버전은 올리지 않는다.
///
/// 연결은 데몬이 닫기도 한다: `BYE`를 받았을 때, 그리고 연결이 묶인 터미널을 `KILL`했을 때(`LIST_REPLY`를 보낸 뒤)다.
/// 앱은 끝난 연결의 읽기 스레드가 끊김(EOF)으로 정리되도록 이 닫힘에 맡긴다.
pub mod kind {
    // -- 앱 -> 데몬 --
    /// `Hello` JSON.
    pub const HELLO: u8 = 1;
    /// `New` JSON.
    pub const NEW: u8 = 2;
    /// `Attach` JSON.
    pub const ATTACH: u8 = 3;
    /// 터미널에 넣을 원시 바이트.
    pub const INPUT: u8 = 4;
    /// `Resize` JSON.
    pub const RESIZE: u8 = 5;
    /// 내용 없음.
    pub const LIST: u8 = 6;
    /// `Kill` JSON.
    pub const KILL: u8 = 7;
    /// 내용 없음.
    pub const SHUTDOWN: u8 = 8;
    /// 내용 없음.
    pub const STATUS_REQUEST: u8 = 9;
    /// 내용 없음. 이 연결을 더 쓰지 않겠다는 표시. 데몬은 답 없이 연결을 닫는다.
    pub const BYE: u8 = 10;

    // -- 데몬 -> 앱 --
    /// `HelloOk` JSON.
    pub const HELLO_OK: u8 = 101;
    /// 오류 문장(글자).
    pub const ERR: u8 = 102;
    /// `List` JSON.
    pub const LIST_REPLY: u8 = 103;
    /// `Attached` JSON.
    pub const ATTACHED: u8 = 104;
    /// 링에 담긴 원시 출력 바이트(재생).
    pub const REPLAY: u8 = 105;
    /// 내용 없음. 재생이 끝났다는 표시.
    pub const REPLAY_END: u8 = 106;
    /// 새로 나온 원시 출력 바이트.
    pub const OUTPUT: u8 = 107;
    /// 종료 코드(글자). 코드를 알 수 없으면 `unknown`.
    pub const EXITED: u8 = 108;
    /// `Status` JSON.
    pub const STATUS: u8 = 109;
    /// 내용 없음. 이어지는 `REPLAY`…`REPLAY_END`가 화면 전체를 대신한다는 표시.
    /// 이 연결로 보내지 못한 출력이 있어, 그 사이에 받은 화면은 버려야 한다.
    pub const RESYNC: u8 = 110;
    /// 이 연결이 맡던 터미널의 출력을 다른 연결이 이어받았다는 알림(내용 없음). 이 터미널의 출력은 더 오지 않는다.
    pub const DETACHED: u8 = 111;
    /// `TerminalState` JSON. 이 연결이 맡은 터미널의 폴더·앞쪽 프로그램·활동 상태가 바뀌었다는 알림.
    pub const TERMINAL_STATE: u8 = 112;
}

/// 연결 맨 처음에 앱이 보내는 인사. 토큰은 `daemon.json`에서 읽은 값이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub token: String,
    pub protocol_version: u32,
}

/// 인사가 거부된 이유. 앱이 어느 쪽이 틀렸는지 알 수 있도록 오류 프레임의 문장이 된다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloError {
    /// 앱과 데몬의 프로토콜 버전이 다르다.
    UnsupportedVersion,
    /// 토큰이 `daemon.json`의 값과 다르다.
    BadToken,
}

impl std::fmt::Display for HelloError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedVersion => "Unsupported protocol version",
            Self::BadToken => "Bad token",
        })
    }
}

impl std::error::Error for HelloError {}

impl Hello {
    /// 인사의 버전과 토큰이 기대와 같은지 확인한다. 어느 쪽이 틀렸는지 오류로 돌려준다.
    ///
    /// 토큰은 어디서 달라지는지와 무관한 시간으로 비교한다. 둘 다 틀리면 버전 오류가 먼저다.
    ///
    /// Examples
    /// --------
    /// | 버전 | 토큰 | 결과                             |
    /// |------|------|----------------------------------|
    /// | 같음 | 같음 | `Ok(())`                         |
    /// | 같음 | 다름 | `Err(HelloError::BadToken)`      |
    /// | 다름 | (무관) | `Err(HelloError::UnsupportedVersion)` |
    pub fn check(
        &self,
        expected_token: &str,
        expected_version: u32,
    ) -> Result<(), HelloError> {
        let given = self.token.as_bytes();
        let expected = expected_token.as_bytes();
        let mut diff = given.len() ^ expected.len();
        for (index, byte) in expected.iter().enumerate() {
            diff |= usize::from(byte ^ given.get(index).copied().unwrap_or(0));
        }
        if self.protocol_version != expected_version {
            Err(HelloError::UnsupportedVersion)
        } else if diff != 0 {
            Err(HelloError::BadToken)
        } else {
            Ok(())
        }
    }
}

/// 인사를 받아들인 데몬의 응답. 앱은 `fingerprint`로 데몬이 자기 버전과 같은지 본다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub pid: u32,
    pub fingerprint: String,
}

/// 새 터미널 요청. `cwd`가 없으면 데몬의 기본 폴더에서 시작한다.
///
/// `wsl_distribution`은 `shell`이 `wsl`일 때 쓸 배포판 이름이고, 없으면 WSL의 기본 배포판이다.
/// `launch`가 있으면 `shell` 대신 그 명세로 프로그램을 직접 띄운다(`shell`은 목록에 보이는 이름으로만 남는다).
/// 필드가 없는 옛 요청도 `None`으로 읽히므로 프로토콜 버전은 올리지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct New {
    pub shell: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub wsl_distribution: Option<String>,
    #[serde(default)]
    pub launch: Option<Launch>,
    /// 되살리는 터미널의 첫 화면(이전 화면 꼬리와 안내 줄). 셸이 내는 출력보다 먼저 터미널 기록(링)에 들어가
    /// 앱이 다시 붙어도 같은 화면이 재생된다. 새 터미널이면 None이다.
    #[serde(default)]
    pub preload: Option<String>,
    /// 앱이 이 요청을 보낼 때 가진 환경 변수(PATH·계정 폴더 등 `shared_environment`가 고른 것). 새 터미널이 데몬 자신의 낡은
    /// 환경 대신 이 값을 받는다. `launch.env`와 겹치면 `launch.env`가 앞선다. 필드가 없는 옛 요청은 빈 목록이다.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// 이미 있는 터미널에 붙는 요청. `cols`·`rows`가 0이면 데몬이 가진 크기를 유지한다(재접속하는 앱은 그 크기로 격자를 만든다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attach {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
}

/// 붙어 있는 터미널의 크기 변경.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resize {
    pub cols: u16,
    pub rows: u16,
}

/// 터미널을 끝내는 요청. 성공하면 남은 터미널의 `List`가 답으로 온다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kill {
    pub id: u32,
}

/// 붙기에 성공한 터미널의 현재 상태. 이 뒤로 `REPLAY`·`REPLAY_END`·`OUTPUT`이 이어진다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attached {
    pub id: u32,
    pub pid: u32,
    pub cols: u16,
    pub rows: u16,
    pub alive: bool,
}

/// 터미널 하나의 폴더·앞쪽 프로그램·에이전트 활동. 데몬이 판정하고 앱은 그대로 보인다.
///
/// `activity`는 앞쪽 프로그램이 에이전트일 때만 값이 있고(에이전트가 아니면 `None`), 프로그램을 아직 모르면 `Unknown`이다.
/// `finished`는 에이전트의 긴 작업이 끝났다고 알리는 프레임에서만 참이다. `account`는 앞쪽 프로그램이 claude·codex일 때 그 계정 범위다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalState {
    pub id: u32,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub is_agent: bool,
    #[serde(default)]
    pub activity: Option<ActivityState>,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub account: Option<Scope>,
}

/// 데몬이 쥔 터미널 하나의 요약.
///
/// `exit_code`는 셸이 끝났고(`alive`가 거짓) 종료 코드를 알 때만 있다. 살아 있거나 코드를 알 수 없으면 `None`이다.
/// `state`는 데몬이 아직 판정하지 못했으면 `None`이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    pub id: u32,
    pub pid: u32,
    pub cols: u16,
    pub rows: u16,
    pub alive: bool,
    pub shell: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub state: Option<TerminalState>,
    /// 계정 버튼으로 연 터미널을 연 계정. 일반 터미널이면 None이다(실행 중인 도구의 계정은 `state.account`).
    #[serde(default)]
    pub account: Option<AccountProfile>,
}

/// 데몬이 쥔 터미널 전체. `LIST`와 `KILL`의 답이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub terminals: Vec<TerminalInfo>,
}

/// 데몬의 pid와 터미널 수. `STATUS_REQUEST`의 답이고 `SHUTDOWN`의 답(터미널 0개)이기도 하다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub pid: u32,
    pub n_terminals: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;

    fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
        let frame = Frame::json(kind::STATUS, &value);
        assert_eq!(frame.parse_json::<T>().unwrap(), value);
    }

    #[test]
    fn test_messages_roundtrip() {
        roundtrip(Hello { token: "abc".into(), protocol_version: PROTOCOL_VERSION });
        roundtrip(HelloOk { pid: 42, fingerprint: "ff".into() });
        roundtrip(New {
            shell: "bash".into(),
            cwd: Some("/tmp/a b".into()),
            cols: 80,
            rows: 24,
            wsl_distribution: None,
            launch: None,
            preload: None,
            env: Vec::new(),
        });
        roundtrip(New {
            shell: "wsl".into(),
            cwd: None,
            cols: 1,
            rows: 1,
            wsl_distribution: Some("Debian".into()),
            launch: Some(Launch {
                program: "claude".into(),
                args: vec!["--resume".into()],
                env: vec![("CLAUDE_CONFIG_DIR".into(), "/x".into())],
                env_remove: vec!["ANTHROPIC_API_KEY".into()],
                cwd: Some("/work".into()),
                title: Some("Main".into()),
                account: None,
            }),
            preload: Some("screen\r\n".into()),
            env: vec![("PATH".into(), "/new/bin".into())],
        });
        roundtrip(TerminalState {
            id: 3,
            cwd: Some("/work".into()),
            program: Some("claude".into()),
            is_agent: true,
            activity: Some(ActivityState::Working),
            finished: true,
            account: Some(Scope::provider("claude")),
        });
        roundtrip(TerminalState {
            id: 4,
            cwd: None,
            program: None,
            is_agent: false,
            activity: None,
            finished: false,
            account: None,
        });
        roundtrip(Attach { id: 3, cols: 100, rows: 30 });
        roundtrip(Resize { cols: 120, rows: 40 });
        roundtrip(Kill { id: 7 });
        roundtrip(Attached { id: 1, pid: 99, cols: 80, rows: 24, alive: true });
        roundtrip(List {
            terminals: vec![
                TerminalInfo {
                    id: 1,
                    pid: 5,
                    cols: 80,
                    rows: 24,
                    alive: false,
                    shell: "cmd".into(),
                    exit_code: Some(7),
                    state: None,
                    account: Some(AccountProfile {
                        id: "00000000-0000-4000-8000-000000000001".into(),
                        provider: "claude".into(),
                        label: "Main".into(),
                        runtime: paddock_core::agent_account::Runtime::Native,
                        wsl_distribution: None,
                    }),
                },
                TerminalInfo {
                    id: 2,
                    pid: 6,
                    cols: 80,
                    rows: 24,
                    alive: true,
                    shell: "bash".into(),
                    exit_code: None,
                    state: Some(TerminalState {
                        id: 2,
                        cwd: Some("/home/me".into()),
                        program: Some("bash".into()),
                        is_agent: false,
                        activity: None,
                        finished: false,
                        account: None,
                    }),
                    account: None,
                },
            ],
        });
        roundtrip(Status { pid: 1, n_terminals: 2 });
    }

    #[test]
    fn test_new_without_optional_fields_parses_as_none() {
        let frame = Frame::new(kind::NEW, br#"{"shell":"bash","cols":80,"rows":24}"#.to_vec());
        let request = frame.parse_json::<New>().unwrap();
        assert_eq!((request.cwd, request.wsl_distribution, request.launch), (None, None, None));
    }

    #[test]
    fn test_terminal_info_without_state_parses_as_none() {
        let json = br#"{"id":1,"pid":2,"cols":80,"rows":24,"alive":true,"shell":"bash"}"#;
        let info = Frame::new(kind::LIST_REPLY, json.to_vec()).parse_json::<TerminalInfo>().unwrap();
        assert_eq!(info.state, None);
    }

    #[test]
    fn test_terminal_state_without_optional_fields_parses_with_safe_defaults() {
        let frame = Frame::new(kind::TERMINAL_STATE, br#"{"id":9}"#.to_vec());
        let state = frame.parse_json::<TerminalState>().unwrap();
        assert_eq!(
            state,
            TerminalState {
                id: 9,
                cwd: None,
                program: None,
                is_agent: false,
                activity: None,
                finished: false,
                account: None
            }
        );
    }

    #[test]
    fn test_terminal_info_without_exit_code_parses_as_none() {
        let json = br#"{"id":1,"pid":2,"cols":80,"rows":24,"alive":true,"shell":"bash"}"#;
        let info = Frame::new(kind::LIST_REPLY, json.to_vec()).parse_json::<TerminalInfo>().unwrap();
        assert_eq!(info.exit_code, None);
    }

    #[test]
    fn test_kinds_are_unique() {
        let kinds = [
            kind::HELLO,
            kind::NEW,
            kind::ATTACH,
            kind::INPUT,
            kind::RESIZE,
            kind::LIST,
            kind::KILL,
            kind::SHUTDOWN,
            kind::STATUS_REQUEST,
            kind::BYE,
            kind::HELLO_OK,
            kind::ERR,
            kind::LIST_REPLY,
            kind::ATTACHED,
            kind::REPLAY,
            kind::REPLAY_END,
            kind::OUTPUT,
            kind::EXITED,
            kind::STATUS,
            kind::RESYNC,
            kind::DETACHED,
            kind::TERMINAL_STATE,
        ];
        let unique: std::collections::HashSet<u8> = kinds.iter().copied().collect();
        assert_eq!(unique.len(), kinds.len());
    }

    #[test]
    fn test_c_t1_token_mismatch_detected() {
        let hello = Hello { token: "secret-token".into(), protocol_version: PROTOCOL_VERSION };
        assert_eq!(hello.check("secret-token", PROTOCOL_VERSION), Ok(()));
        for wrong in ["secret-tokeN", "secret-token-longer", ""] {
            assert_eq!(hello.check(wrong, PROTOCOL_VERSION), Err(HelloError::BadToken), "{wrong:?}");
        }
        // 버전이 다르면 토큰이 맞아도 버전 오류다. 둘 다 틀리면 버전 오류가 먼저 나온다.
        assert_eq!(hello.check("secret-token", PROTOCOL_VERSION + 1), Err(HelloError::UnsupportedVersion));
        assert_eq!(hello.check("other", PROTOCOL_VERSION + 1), Err(HelloError::UnsupportedVersion));
    }

    #[test]
    fn test_hello_error_messages_tell_which_side_is_wrong() {
        assert_eq!(HelloError::BadToken.to_string(), "Bad token");
        assert_eq!(HelloError::UnsupportedVersion.to_string(), "Unsupported protocol version");
    }
}
