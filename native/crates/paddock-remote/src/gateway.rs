//! 게이트웨이의 공유 상태: 기대 값, 기기 목록, 페어링 창, 열린 연결. HTTP 처리와 제어 소켓이 같은 값을 본다.
use std::io;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use paddock_protocol::remote_control::{DeviceInfo, GatewayStatus, PairingOpened};

use crate::connections::Connections;
use crate::devices::Devices;
use crate::gate::GateConfig;
use crate::log::{self, Event};
use crate::pairing::Pairing;

/// 게이트웨이가 끝나는 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    DaemonLost,
    ControlStop,
}

impl StopReason {
    /// 기록에 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            StopReason::DaemonLost => "daemon connection lost",
            StopReason::ControlStop => "stop requested",
        }
    }
}

/// 거부 기록의 흘러넘침 방지 규칙: 한 구간에 이만큼만 쓰고 나머지는 센다.
pub struct DenialLogRule;

impl DenialLogRule {
    pub const WINDOW: Duration = Duration::from_secs(10);
    pub const MAX_LINES_PER_WINDOW: u32 = 20;
}

/// 거부 기록 구간 하나의 계수.
struct DenialWindow {
    started: Instant,
    n_written: u32,
    n_suppressed: u32,
}

pub struct Gateway {
    pub config: GateConfig,
    pub devices: Devices,
    pub pairing: Pairing,
    pub connections: Connections,
    pub port: u16,
    pub daemon_pid: u32,
    pub daemon_compatible: bool,
    stop: Sender<StopReason>,
    denial_window: Mutex<DenialWindow>,
}

impl Gateway {
    pub fn new(
        config: GateConfig,
        devices: Devices,
        port: u16,
        daemon_pid: u32,
        daemon_compatible: bool,
        stop: Sender<StopReason>,
    ) -> Self {
        Self {
            config,
            devices,
            pairing: Pairing::default(),
            connections: Connections::default(),
            port,
            daemon_pid,
            daemon_compatible,
            stop,
            denial_window: Mutex::new(DenialWindow { started: Instant::now(), n_written: 0, n_suppressed: 0 }),
        }
    }

    /// 지금 시각(유닉스 초).
    pub fn now_secs() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
    }

    /// 앱에 보여 줄 지금 상태.
    pub fn status(&self) -> GatewayStatus {
        GatewayStatus {
            pid: std::process::id(),
            port: self.port,
            expected_host: self.config.expected_host.clone(),
            expected_login: self.config.expected_login.clone(),
            daemon_compatible: self.daemon_compatible,
            n_devices: u32::try_from(self.devices.active(Self::now_secs()).len()).unwrap_or(u32::MAX),
            n_connections: u32::try_from(self.connections.count()).unwrap_or(u32::MAX),
            pairing_open: self.pairing.remaining(Instant::now()).is_some(),
        }
    }

    /// 기기 승인 창을 새로 연다.
    pub fn open_pairing(&self) -> PairingOpened {
        let now = Instant::now();
        let code = self.pairing.open(now);
        log::write(&Event::PairingOpened);
        PairingOpened { code, expires_in_secs: self.pairing.remaining(now).map_or(0, |remaining| remaining.as_secs()) }
    }

    /// 폐기·만료되지 않은 기기 목록.
    pub fn devices_info(&self) -> Vec<DeviceInfo> {
        self.devices.active(Self::now_secs())
    }

    /// 기기를 폐기하고 그 기기의 열린 연결을 끊는다. 폐기할 기기가 없었으면 `false`다.
    pub fn revoke(
        &self,
        id: &str,
    ) -> io::Result<bool> {
        let revoked = self.devices.revoke(id, Self::now_secs())?;
        if let Some(name) = &revoked {
            self.connections.close_device(id);
            log::write(&Event::DeviceRevoked { device: name });
        }
        Ok(revoked.is_some())
    }

    /// 게이트웨이를 끝내 달라고 main에 알린다. 이미 끝나는 중이면 무시한다.
    pub fn request_stop(
        &self,
        reason: StopReason,
    ) {
        let _ = self.stop.send(reason);
    }

    /// 거부 한 건을 기록한다. 한 구간에 `MAX_LINES_PER_WINDOW`줄을 넘으면 세기만 하고, 다음 구간 첫 줄 앞에 몇 건을 건너뛰었는지 남긴다.
    pub fn log_denial(
        &self,
        route: &str,
        reason: &str,
    ) {
        let mut window = self.denial_window.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        if now.duration_since(window.started) >= DenialLogRule::WINDOW {
            if window.n_suppressed > 0 {
                log::write(&Event::DenialsSuppressed { n_denials: window.n_suppressed });
            }
            *window = DenialWindow { started: now, n_written: 0, n_suppressed: 0 };
        }
        if window.n_written < DenialLogRule::MAX_LINES_PER_WINDOW {
            window.n_written += 1;
            log::write(&Event::Denied { route, reason });
        } else {
            window.n_suppressed += 1;
        }
    }
}
