//! 앱↔데몬 메시지: 프레임 종류 번호와 JSON 내용의 구조체.
/// 셸 대신 직접 띄우는 프로그램의 실행 명세. 규칙은 core의 `account_launch`가 쥔다.
pub use paddock_core::account_launch::Launch;
use paddock_core::agent_account::{AccountProfile, Scope};
use paddock_core::agent_model::{ActivityState, StatusState};
use serde::{Deserialize, Serialize};

/// 앱과 데몬이 맞춰야 하는 프로토콜 버전. 데몬은 인사(`HELLO_OK`)로 자기 버전을 알리고, 앱이 그 값을 자기 값과 비교해
/// 같은 터미널에 계속 붙을지 정한다.
///
/// 인사·종료 틀은 어떤 버전에서도 번호와 모양을 바꾸지 않는다(버전이 다른 데몬도 앱이 인사하고 끝낼 수 있어야 하기 때문이다):
/// `HELLO 1`·`Hello{token, protocol_version}`, `HELLO_OK 101`·`HelloOk{pid, fingerprint, protocol_version}`, `ERR 102`(글자),
/// `STATUS_REQUEST 9`, `STATUS 109`·`Status{pid, n_terminals}`, `SHUTDOWN 8`, `BYE 10`. 이 틀에는 선택 필드도 더하지 않는다 — 옛 앱·데몬이 읽는 모양이 그대로여야 하고,
/// 시험은 이 틀의 직렬화 전문을 그대로 대조한다(`protocol_version` 추가가 유일한 예외였다).
///
/// 올린다:
/// - 필수 필드를 더하거나 빼거나 타입을 바꾼다.
/// - 기존 kind의 뜻이나 그 요청의 답 kind를 바꾼다.
/// - 앱이 답을 기다리는 새 앱 → 데몬 요청 kind를 더한다(옛 데몬은 모르는 요청에 답하지 않는다).
///
/// 올리지 않는다:
/// - `serde(default)` 선택 필드를 더한다(없으면 `None`·기본값으로 읽힌다).
/// - 데몬 → 앱 알림 kind를 더한다(앱은 모르는 kind를 버린다).
/// - 필드의 뜻이 그대로인 문서 변경.
///
/// 모양은 `protocol-shape.v<버전>.txt`가 기록하고 시험이 지금 모양과 대조한다. 올리면 새 버전의 파일을 만들고 옛 파일은 지운다.
/// 올리지 않는 변경은 같은 파일을 갱신하고 머리 `compatible-changes:` 줄에 날짜와 호환으로 보는 이유를 적는다.
pub const PROTOCOL_VERSION: u32 = 2;

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
/// 토큰이 맞으면 앱의 프로토콜 버전이 데몬과 달라도 데몬은 `HELLO_OK`로 자기 버전을 알린다. 버전이 다른 연결은
/// 인사만 받아들인 연결이라 STATUS_REQUEST·SHUTDOWN·BYE만 처리하고, 나머지 요청에는 `ERR`로 답하되 연결은 닫지 않는다
/// (앱이 이어서 종료를 요청할 수 있게).
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
    /// 내용 없음. WSL 메모리를 읽어 `MEMORY_REPORT`로 답하라는 요청(요청 때만 읽는다).
    pub const MEMORY_REQUEST: u8 = 11;
    /// `StopAgent` JSON. 터미널에서 도는 에이전트 CLI 하나(셸이 아니라)를 검증한 뒤 멈추라는 요청. `STOP_AGENT_REPLY`로 답한다.
    pub const STOP_AGENT: u8 = 12;
    /// `CheckIdle` JSON. 터미널 앞쪽이 지금 셸 자신인지 다시 읽어 확인하라는 요청. `CHECK_IDLE_REPLY`로 답한다.
    pub const CHECK_IDLE: u8 = 13;

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
    /// `MemoryReport` JSON. `MEMORY_REQUEST`의 답.
    pub const MEMORY_REPORT: u8 = 113;
    /// `StopAgentReply` JSON. `STOP_AGENT`의 답.
    pub const STOP_AGENT_REPLY: u8 = 114;
    /// `CheckIdleReply` JSON. `CHECK_IDLE`의 답.
    pub const CHECK_IDLE_REPLY: u8 = 115;
}

/// 연결 맨 처음에 앱이 보내는 인사. 토큰은 `daemon.json`에서 읽은 값이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub token: String,
    pub protocol_version: u32,
}

/// 인사나 인사 직후 요청이 거부된 이유. 앱이 어느 쪽이 틀렸는지 알 수 있도록 오류 프레임의 문장이 된다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelloError {
    /// 앱과 데몬의 프로토콜 버전이 다르다. 데몬은 인사만 받아들인 연결에서 허용하지 않은 요청에 이 문장으로 답한다.
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
    /// 인사의 토큰이 기대와 같은지 확인한다. 버전은 보지 않는다 — 버전이 달라도 토큰이 맞으면 인사는 받아들이고,
    /// 같은 터미널에 계속 붙을지는 호출자가 `HelloOk.protocol_version`을 자기 버전과 비교해 정한다.
    ///
    /// 토큰은 어디서 달라지는지와 무관한 시간으로 비교한다.
    ///
    /// Examples
    /// --------
    /// | 토큰 | 결과                        |
    /// |------|-----------------------------|
    /// | 같음 | `Ok(())`                    |
    /// | 다름 | `Err(HelloError::BadToken)` |
    pub fn check_token(
        &self,
        expected_token: &str,
    ) -> Result<(), HelloError> {
        let given = self.token.as_bytes();
        let expected = expected_token.as_bytes();
        let mut diff = given.len() ^ expected.len();
        for (index, byte) in expected.iter().enumerate() {
            diff |= usize::from(byte ^ given.get(index).copied().unwrap_or(0));
        }
        if diff != 0 { Err(HelloError::BadToken) } else { Ok(()) }
    }
}

/// 인사를 받아들인 데몬의 응답. 앱은 `protocol_version`으로 계속 붙을 수 있는지, `fingerprint`로 데몬이 자기 실행 파일과 같은지 본다.
///
/// `protocol_version`이 없는 응답은 이 필드를 모르는 옛 데몬의 것이라 0으로 읽힌다(앱은 0을 첫 버전으로 본다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub pid: u32,
    pub fingerprint: String,
    #[serde(default)]
    pub protocol_version: u32,
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
/// `signal`은 에이전트가 터미널 제어 시퀀스(OSC 7501)로 직접 알린 상태다. 앞쪽 프로그램이 에이전트이고 알린 적이 있을 때만 있고,
/// 있으면 `activity`(입력·출력으로 추정한 상태)보다 정확한 사실이다. 이 필드를 모르는 옛 데몬은 보내지 않아 `None`으로 읽힌다.
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
    /// 앞쪽에서 도는 에이전트가 이어 가고 있는 대화. 대화를 아직 확신할 수 없으면 None이다.
    #[serde(default)]
    pub conversation: Option<Conversation>,
    #[serde(default)]
    pub signal: Option<Signal>,
}

/// 에이전트가 알린 상태. `kind`는 `blocked`의 이유(`permission`·`question`·`auth`) 같은 보조 값이다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub state: StatusState,
    #[serde(default)]
    pub kind: Option<String>,
}

/// 터미널 앞쪽에서 도는 에이전트(CLI)가 지금 이어 가고 있는 대화와 그 프로세스.
///
/// 앱이 "같은 대화를 다른 계정으로"를 하려고 쓴다. `pid`·`start_mark`는 `STOP_AGENT`가 같은 프로세스인지 확인하는 표지이고,
/// 대화 내용은 담지 않는다(식별자와 폴더만).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    /// `claude`·`codex`.
    pub provider: String,
    pub session_id: String,
    pub cwd: String,
    /// 그 에이전트가 쓰는 설정 폴더. 환경을 읽지 못했으면 None이다.
    #[serde(default)]
    pub config_dir: Option<String>,
    pub pid: u32,
    /// 프로세스 시작 표지. 같은 환경에서 읽은 값끼리만 같은지 비교한다: Linux·Windows 네이티브는 시작 시각(ms, 1970년 기준),
    /// WSL은 부팅 뒤 시작 tick(`/proc/<pid>/stat` 22번째 칸). 모르면 None.
    #[serde(default)]
    pub start_mark: Option<u64>,
}

/// 터미널의 에이전트 CLI를 멈추는 요청. `pid`·`start_mark`가 데몬이 지금 보고 있는 그 터미널의 에이전트와 같을 때만 멈춘다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopAgent {
    pub id: u32,
    pub pid: u32,
    /// `Conversation.start_mark`와 같은 표지.
    #[serde(default)]
    pub start_mark: Option<u64>,
}

/// `STOP_AGENT`의 답: 멈췄는지와 사용자에게 보일 문장(멈췄으면 비어 있다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopAgentReply {
    pub id: u32,
    pub stopped: bool,
    #[serde(default)]
    pub message: String,
}

/// 터미널 앞쪽이 셸 자신인지 지금 다시 읽어 달라는 요청. 저장해 둔 상태는 몇 초 늦을 수 있어 쓰지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckIdle {
    pub id: u32,
}

/// `CHECK_IDLE`의 답: 앞쪽이 셸 자신이면 `idle`이고, 아니면 사용자에게 보일 이유 문장이 있다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckIdleReply {
    pub id: u32,
    pub idle: bool,
    #[serde(default)]
    pub message: String,
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

/// `MEMORY_REQUEST`의 답: WSL 가상 머신의 메모리와 터미널별 메모리. WSL이 없거나 읽지 못했으면 `wsl`이 없다.
/// 이 컴퓨터 전체 메모리는 앱이 직접 읽으므로 담지 않는다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryReport {
    pub wsl: Option<paddock_core::memory_guard::WslMemory>,
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
        roundtrip(HelloOk { pid: 42, fingerprint: "ff".into(), protocol_version: PROTOCOL_VERSION });
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
            conversation: None,
            signal: Some(Signal { state: StatusState::Blocked, kind: Some("permission".into()) }),
        });
        roundtrip(TerminalState {
            id: 4,
            cwd: None,
            program: None,
            is_agent: false,
            activity: None,
            finished: false,
            account: None,
            conversation: None,
            signal: None,
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
                        conversation: None,
                        signal: None,
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
                account: None,
                conversation: None,
                signal: None,
            }
        );
    }

    #[test]
    fn test_terminal_info_without_exit_code_parses_as_none() {
        let json = br#"{"id":1,"pid":2,"cols":80,"rows":24,"alive":true,"shell":"bash"}"#;
        let info = Frame::new(kind::LIST_REPLY, json.to_vec()).parse_json::<TerminalInfo>().unwrap();
        assert_eq!(info.exit_code, None);
    }

    /// 모든 프레임 종류의 (이름, 번호). 중복 검사·모양 기록·누락 검사가 모두 이 목록 하나에서 나온다.
    const KINDS: &[(&str, u8)] = &[
        ("HELLO", kind::HELLO),
        ("NEW", kind::NEW),
        ("ATTACH", kind::ATTACH),
        ("INPUT", kind::INPUT),
        ("RESIZE", kind::RESIZE),
        ("LIST", kind::LIST),
        ("KILL", kind::KILL),
        ("SHUTDOWN", kind::SHUTDOWN),
        ("STATUS_REQUEST", kind::STATUS_REQUEST),
        ("BYE", kind::BYE),
        ("MEMORY_REQUEST", kind::MEMORY_REQUEST),
        ("STOP_AGENT", kind::STOP_AGENT),
        ("CHECK_IDLE", kind::CHECK_IDLE),
        ("HELLO_OK", kind::HELLO_OK),
        ("ERR", kind::ERR),
        ("LIST_REPLY", kind::LIST_REPLY),
        ("ATTACHED", kind::ATTACHED),
        ("REPLAY", kind::REPLAY),
        ("REPLAY_END", kind::REPLAY_END),
        ("OUTPUT", kind::OUTPUT),
        ("EXITED", kind::EXITED),
        ("STATUS", kind::STATUS),
        ("RESYNC", kind::RESYNC),
        ("DETACHED", kind::DETACHED),
        ("TERMINAL_STATE", kind::TERMINAL_STATE),
        ("MEMORY_REPORT", kind::MEMORY_REPORT),
        ("STOP_AGENT_REPLY", kind::STOP_AGENT_REPLY),
        ("CHECK_IDLE_REPLY", kind::CHECK_IDLE_REPLY),
    ];

    /// 어떤 버전에서도 바뀌지 않는 인사·종료 틀의 종류 이름.
    const HANDSHAKE_KINDS: &[&str] = &["HELLO", "HELLO_OK", "ERR", "STATUS_REQUEST", "STATUS", "SHUTDOWN", "BYE"];

    /// 지금 버전의 모양 기록. 파일 이름의 버전이 `PROTOCOL_VERSION`과 다르면 `shape_matches_the_snapshot`이 실패한다.
    const SHAPE_FILE: &str = include_str!("../protocol-shape.v2.txt");

    /// 인사·종료 틀의 기록. 버전이 올라도 이 파일은 바뀌지 않는다.
    const HANDSHAKE_FILE: &str = include_str!("../protocol-shape.handshake.txt");

    fn json_of<T: Serialize>(value: &T) -> String {
        serde_json::to_string_pretty(value).unwrap()
    }

    /// 인사·종료 틀의 지금 모양: 종류 번호와 메시지 표본.
    fn handshake_shape() -> String {
        let mut text = String::from("[handshake kinds]\n");
        for name in HANDSHAKE_KINDS {
            let (_, number) = KINDS.iter().find(|(kind_name, _)| kind_name == name).expect("handshake kind in KINDS");
            text += &format!("{name} {number}\n");
        }
        text += "ERR carries plain text\n[handshake messages]\n";
        text += &format!("Hello {}\n", json_of(&Hello { token: "token".into(), protocol_version: 1 }));
        text += &format!("HelloOk {}\n", json_of(&HelloOk { pid: 1, fingerprint: "ff".into(), protocol_version: 1 }));
        text += &format!("Status {}\n", json_of(&Status { pid: 1, n_terminals: 1 }));
        text
    }

    /// 지금 모양 전체: 버전 + 인사·종료 틀 + 모든 종류 번호 + 모든 선택 필드를 채운 메시지 표본.
    fn shape() -> String {
        let profile = AccountProfile {
            id: "00000000-0000-4000-8000-000000000001".into(),
            provider: "claude".into(),
            label: "Main".into(),
            runtime: paddock_core::agent_account::Runtime::Wsl,
            wsl_distribution: Some("Debian".into()),
        };
        let state = TerminalState {
            id: 3,
            cwd: Some("/work".into()),
            program: Some("claude".into()),
            is_agent: true,
            activity: Some(ActivityState::Working),
            finished: true,
            account: Some(Scope {
                provider: "claude".into(),
                profile: Some(profile.clone()),
                custom: true,
                linked: Some(paddock_core::agent_account::Linked { id: "linked".into(), label: "Main".into() }),
            }),
            conversation: Some(Conversation {
                provider: "claude".into(),
                session_id: "session".into(),
                cwd: "/work".into(),
                config_dir: Some("/config".into()),
                pid: 4,
                start_mark: Some(9),
            }),
            signal: Some(Signal { state: StatusState::Blocked, kind: Some("permission".into()) }),
        };
        let launch = Launch {
            program: "claude".into(),
            args: vec!["--resume".into()],
            env: vec![("NAME".into(), "value".into())],
            env_remove: vec!["REMOVED".into()],
            cwd: Some("/work".into()),
            title: Some("Main".into()),
            account: Some(profile.clone()),
        };
        let mut text = format!("version: {PROTOCOL_VERSION}\n\n{}\n[kinds]\n", handshake_shape());
        for (name, number) in KINDS {
            text += &format!("{name} {number}\n");
        }
        text += "\n[messages]\n";
        text += &format!(
            "New {}\n",
            json_of(&New {
                shell: "bash".into(),
                cwd: Some("/work".into()),
                cols: 80,
                rows: 24,
                wsl_distribution: Some("Debian".into()),
                launch: Some(launch),
                preload: Some("screen".into()),
                env: vec![("PATH".into(), "/bin".into())],
            })
        );
        text += &format!("Attach {}\n", json_of(&Attach { id: 3, cols: 80, rows: 24 }));
        text += &format!("Resize {}\n", json_of(&Resize { cols: 80, rows: 24 }));
        text += &format!("Kill {}\n", json_of(&Kill { id: 3 }));
        text += &format!("Attached {}\n", json_of(&Attached { id: 3, pid: 5, cols: 80, rows: 24, alive: true }));
        text += &format!("TerminalState {}\n", json_of(&state));
        text += &format!("StopAgent {}\n", json_of(&StopAgent { id: 3, pid: 4, start_mark: Some(9) }));
        text += &format!("StopAgentReply {}\n", json_of(&StopAgentReply { id: 3, stopped: true, message: "m".into() }));
        text += &format!("CheckIdle {}\n", json_of(&CheckIdle { id: 3 }));
        text += &format!("CheckIdleReply {}\n", json_of(&CheckIdleReply { id: 3, idle: true, message: "m".into() }));
        text += &format!(
            "List {}\n",
            json_of(&List {
                terminals: vec![TerminalInfo {
                    id: 3,
                    pid: 5,
                    cols: 80,
                    rows: 24,
                    alive: false,
                    shell: "bash".into(),
                    exit_code: Some(1),
                    state: Some(state),
                    account: Some(profile),
                }],
            })
        );
        text += &format!(
            "MemoryReport {}\n",
            json_of(&MemoryReport {
                wsl: Some(paddock_core::memory_guard::WslMemory {
                    total: 4,
                    available: 3,
                    swap_total: 2,
                    swap_used: 1,
                    terminals: [(3, 100)].into_iter().collect(),
                    detached: vec![5],
                }),
            })
        );
        text
    }

    /// 기록 파일의 본문: `#`로 시작하는 머리 줄을 빼고 줄바꿈과 끝 공백을 맞춘다.
    fn body_of(file: &str) -> String {
        let lines: Vec<&str> = file.lines().filter(|line| !line.starts_with('#')).collect();
        lines.join("\n").trim().to_string()
    }

    const SHAPE_CHANGED: &str = "\
The protocol message shape no longer matches its snapshot. Decide first, then update the files:
  1. Raise PROTOCOL_VERSION and create protocol-shape.v<new>.txt (delete the old file) when a required field was
     added, removed or retyped, an existing kind or its reply kind changed meaning, or the app now waits for a reply
     to a new app -> daemon request kind (an older daemon never answers it).
  2. Keep PROTOCOL_VERSION when only a serde(default) optional field or a daemon -> app notification kind was added:
     update the current protocol-shape.v<N>.txt and add a header line
     `# compatible-changes: <date> - what changed and why older peers still work`.
  3. protocol-shape.handshake.txt (hello, hello-ok, err, status request/reply, shutdown, bye) never changes.
     If this test fails on it, undo the change instead of updating the file.
The current shape follows. For 1 or 2, paste it into the snapshot file after its `#` header lines.";

    #[test]
    fn test_kinds_are_unique() {
        let unique: std::collections::HashSet<u8> = KINDS.iter().map(|(_, number)| *number).collect();
        assert_eq!(unique.len(), KINDS.len());
        let names: std::collections::HashSet<&str> = KINDS.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), KINDS.len());
    }

    #[test]
    fn test_kinds_list_covers_every_kind() {
        // `kind` 모듈 본문의 `pub const NAME: u8`을 읽어, 상수를 더하고 KINDS에 빠뜨리면 실패하게 한다.
        let source = include_str!("message.rs");
        let start = source.find("pub mod kind {").expect("kind module");
        let module = &source[start..];
        let module = &module[..module.find("\n}\n").expect("kind module end")];
        let declared: std::collections::BTreeSet<&str> = module
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("pub const "))
            .filter_map(|rest| rest.split(':').next())
            .collect();
        let listed: std::collections::BTreeSet<&str> = KINDS.iter().map(|(name, _)| *name).collect();
        assert_eq!(declared, listed, "every constant in `kind` must be listed in KINDS");
    }

    #[test]
    fn test_protocol_shape_matches_snapshot_for_this_version() {
        let (actual, recorded) = (shape(), body_of(SHAPE_FILE));
        assert!(
            actual.trim() == recorded,
            "{SHAPE_CHANGED}\n\n(PROTOCOL_VERSION is {PROTOCOL_VERSION}; the file name must carry the same number)\n\n{actual}"
        );
    }

    #[test]
    fn test_handshake_frames_never_change() {
        let (actual, recorded) = (handshake_shape(), body_of(HANDSHAKE_FILE));
        assert!(actual.trim() == recorded, "{SHAPE_CHANGED}\n\n{actual}");
    }

    #[test]
    fn test_terminal_state_carries_the_reported_signal_and_a_peer_without_it_reads_none() {
        let frame = Frame::json(
            kind::TERMINAL_STATE,
            &TerminalState {
                id: 1,
                cwd: None,
                program: Some("claude".into()),
                is_agent: true,
                activity: None,
                finished: false,
                account: None,
                conversation: None,
                signal: Some(Signal { state: StatusState::Blocked, kind: Some("permission".into()) }),
            },
        );
        assert_eq!(
            frame.as_text(),
            r#"{"id":1,"cwd":null,"program":"claude","is_agent":true,"activity":null,"finished":false,"account":null,"conversation":null,"signal":{"state":"blocked","kind":"permission"}}"#
        );
        // 필드를 모르는 옛 데몬의 프레임은 신호가 없는 것으로 읽힌다.
        let old = Frame::new(kind::TERMINAL_STATE, br#"{"id":1,"is_agent":true}"#.to_vec());
        assert_eq!(old.parse_json::<TerminalState>().unwrap().signal, None);
        // 신호의 kind도 없어도 된다.
        let bare = Frame::new(kind::TERMINAL_STATE, br#"{"id":1,"signal":{"state":"idle"}}"#.to_vec());
        assert_eq!(
            bare.parse_json::<TerminalState>().unwrap().signal,
            Some(Signal { state: StatusState::Idle, kind: None })
        );
    }

    #[test]
    fn test_hello_ok_without_protocol_version_reads_as_zero() {
        let frame = Frame::new(kind::HELLO_OK, br#"{"pid":7,"fingerprint":"ff"}"#.to_vec());
        let hello_ok = frame.parse_json::<HelloOk>().unwrap();
        assert_eq!(hello_ok, HelloOk { pid: 7, fingerprint: "ff".into(), protocol_version: 0 });
    }

    #[test]
    fn test_c_t1_token_mismatch_detected() {
        let hello = Hello { token: "secret-token".into(), protocol_version: PROTOCOL_VERSION };
        assert_eq!(hello.check_token("secret-token"), Ok(()));
        for wrong in ["secret-tokeN", "secret-token-longer", ""] {
            assert_eq!(hello.check_token(wrong), Err(HelloError::BadToken), "{wrong:?}");
        }
        // 버전은 보지 않는다. 다른 버전의 인사도 토큰이 맞으면 통과하고 틀리면 토큰 오류다.
        let other_version = Hello { token: "secret-token".into(), protocol_version: PROTOCOL_VERSION + 1 };
        assert_eq!(other_version.check_token("secret-token"), Ok(()));
        assert_eq!(other_version.check_token("other"), Err(HelloError::BadToken));
    }

    #[test]
    fn test_c_d76_a_memory_report_roundtrips_with_and_without_wsl() {
        use paddock_core::memory_guard::WslMemory;
        roundtrip(MemoryReport::default());
        roundtrip(MemoryReport {
            wsl: Some(WslMemory {
                total: 26,
                available: 2,
                swap_total: 16,
                swap_used: 1,
                terminals: [(3, 100), (5, 7)].into_iter().collect(),
                detached: vec![5],
            }),
        });
        roundtrip(StopAgent { id: 3, pid: 40, start_mark: Some(9) });
        roundtrip(StopAgentReply { id: 3, stopped: false, message: "x".into() });
        roundtrip(CheckIdle { id: 3 });
        roundtrip(CheckIdleReply { id: 3, idle: false, message: "x".into() });
        roundtrip(Conversation {
            provider: "claude".into(),
            session_id: "s".into(),
            cwd: "/w".into(),
            config_dir: None,
            pid: 4,
            start_mark: None,
        });
        const {
            assert!(
                kind::MEMORY_REQUEST < 100
                    && kind::CHECK_IDLE < 100
                    && kind::MEMORY_REPORT > 100
                    && kind::CHECK_IDLE_REPLY > 100,
                "requests go to the daemon, reports come back"
            )
        };
    }

    #[test]
    fn test_hello_error_messages_tell_which_side_is_wrong() {
        assert_eq!(HelloError::BadToken.to_string(), "Bad token");
        assert_eq!(HelloError::UnsupportedVersion.to_string(), "Unsupported protocol version");
    }
}
