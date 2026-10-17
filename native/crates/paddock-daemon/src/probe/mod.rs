//! 터미널 셸의 현재 폴더·앞쪽 프로그램·환경을 운영체제에서 읽는 곳.
//!
//! 운영체제마다 읽는 방법이 다르다: Linux는 `/proc`, Windows는 프로세스 목록과 명령줄 조회, Windows의 WSL 터미널은 WSL 안의 `/proc`.
//! 어느 쪽도 자식 프로세스를 상주시키지 않는다. 읽는 일은 비싸므로 호출자(`activity::tick`)가 필요한 터미널만 모아 한 번에 묻는다.
use std::collections::BTreeMap;

use paddock_core::agent_account::{AccountEnv, Runtime};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod wsl;

/// 조사할 터미널 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeTarget {
    pub id: u32,
    /// 터미널이 띄운 셸 프로세스 번호.
    pub shell_pid: u32,
    /// WSL 터미널이면 그 안의 셸을 찾는 정보.
    pub wsl: Option<WslTarget>,
}

/// WSL 터미널의 셸을 WSL 안에서 찾는 정보.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslTarget {
    /// 터미널을 띄운 배포판. 없으면 WSL의 기본 배포판이다.
    pub distribution: Option<String>,
    /// 터미널이 셸에 물려준 표지 값.
    pub marker: String,
}

/// 조사 원본.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// 셸의 현재 폴더. 운영체제가 알려 주지 않으면 None이다.
    pub cwd: Option<String>,
    /// 앞쪽 프로그램의 명령줄. 셸만 떠 있으면 셸 자신의 명령줄이다.
    pub argv: Vec<String>,
    /// 앞쪽 프로그램의 계정 환경. 읽지 못하면 None이다.
    pub env: Option<AccountEnv>,
    pub runtime: Runtime,
    /// WSL 터미널이면 셸의 배포판 이름, 아니면 빈 글자.
    pub distribution: String,
    /// 앞쪽 프로세스 번호. 알 수 없으면 None.
    pub pid: Option<u32>,
    /// 앞쪽 프로세스의 시작 시각(ms, 1970년 기준). 알 수 없으면 None.
    pub started_at: Option<u64>,
    /// WSL 안에서 읽은 앞쪽 프로세스의 Claude sessions 기록 본문. 읽지 못했으면 None.
    pub session_record: Option<String>,
}

/// 터미널 조사기. 읽지 못한 터미널은 결과에서 빠진다.
pub trait Probe: Send + Sync {
    /// `targets`를 한 번에 읽는다. 키는 터미널 번호다.
    fn read(
        &self,
        targets: &[ProbeTarget],
    ) -> BTreeMap<u32, Reading>;
}

/// 이 운영체제의 조사기. 아직 지원하지 않는 운영체제(macOS)는 아무것도 읽지 못한다.
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn read(
        &self,
        targets: &[ProbeTarget],
    ) -> BTreeMap<u32, Reading> {
        #[cfg(target_os = "linux")]
        let readings = linux::read(targets);
        #[cfg(windows)]
        let readings = windows::read(targets);
        #[cfg(not(any(target_os = "linux", windows)))]
        let readings = {
            let _ = targets;
            BTreeMap::new()
        };
        readings
    }
}
