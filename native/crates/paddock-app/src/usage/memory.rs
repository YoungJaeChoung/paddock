//! 상태 줄 Memory 게이지와 90% 경고: 이 컴퓨터 전체 메모리(직접 읽음)와 WSL 메모리(데몬에 `MEMORY_REQUEST`로 물어 받음) 가운데 더 빡빡한 쪽을 보인다.
//!
//! 요청은 Memory 항목이 켜져 있고 창이 포커스를 가진 동안 10초마다 보낸다. 답은 어느 터미널 연결로 오든 `store_report`가 한곳에 쥔다.
//! 판정과 문장은 core의 `memory_guard`가 정하고, 여기는 값을 모아 게이지와 경고(한 번만, 80% 아래로 내려가야 다시)를 갱신한다.
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use paddock_core::memory_guard::{MemoryDetails, Source, WslMemory, memory_details, tightest_memory, warn_step};
use paddock_protocol::message::MemoryReport;

/// 이 컴퓨터 몫(전체·여유 바이트)과 WSL 몫.
#[cfg(not(feature = "automation"))]
type SystemAndWsl = (Option<(u64, u64)>, Option<WslMemory>);

/// 메모리를 묻는 간격.
pub const REQUEST_INTERVAL: Duration = Duration::from_secs(10);

/// 이 시간보다 오래된 WSL 답은 쓰지 않는다(요청 간격의 세 배: 답이 두 번 걸러져도 값을 유지한다).
/// 기존 앱의 10초와 다르다 — 기존 앱은 자기 스캔 주기와 같은 값이지만, 이 앱은 답이 요청 간격(10초)마다 오므로 10초로 맞추면 답 하나만 늦어도 게이지가 이 컴퓨터 값으로 깜빡인다.
const REPORT_STALE: Duration = Duration::from_secs(30);

static LATEST: Mutex<Option<(Instant, MemoryReport)>> = Mutex::new(None);
static N_REPORTS: AtomicU64 = AtomicU64::new(0);

/// 데몬이 보낸 메모리 답을 쥔다(터미널 연결의 받는 스레드가 부른다).
pub fn store_report(report: MemoryReport) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = Some((Instant::now(), report));
    }
    N_REPORTS.fetch_add(1, Ordering::Relaxed);
}

/// 지금까지 받은 답의 수. 달라졌으면 새 답이 왔다.
pub fn n_reports() -> u64 {
    N_REPORTS.load(Ordering::Relaxed)
}

/// 최근 답의 WSL 몫. 오래됐거나 없으면 None이다.
pub fn latest_wsl() -> Option<WslMemory> {
    let latest = LATEST.lock().ok()?;
    latest.as_ref().filter(|(at, _)| at.elapsed() < REPORT_STALE).and_then(|(_, report)| report.wsl.clone())
}

/// 이 컴퓨터 전체 물리 메모리와 쓸 수 있는 몫(바이트). 읽지 못하면 None이다.
#[cfg(windows)]
pub fn system_memory() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: 0으로 채운 구조체에 크기를 적어 넘긴다. 운영체제가 값을 채운다.
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut status) != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
    }
}

/// 이 컴퓨터 전체 메모리(`/proc/meminfo`의 `MemTotal`·`MemAvailable`, 바이트).
#[cfg(target_os = "linux")]
pub fn system_memory() -> Option<(u64, u64)> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// 다른 운영체제는 아직 읽지 않는다(게이지는 `—`).
#[cfg(not(any(windows, target_os = "linux")))]
pub fn system_memory() -> Option<(u64, u64)> {
    None
}

/// `/proc/meminfo` 글에서 (전체, 쓸 수 있는 몫) 바이트. 둘 중 하나라도 없으면 None이다.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kilobytes = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
    };
    Some((kilobytes("MemTotal:")? * 1024, kilobytes("MemAvailable:")? * 1024))
}

/// 상태 줄 게이지가 보일 값.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gauge {
    pub percent: Option<u32>,
    pub source: Source,
    /// 말풍선 문장. 사용률을 모르면 None이다.
    pub details: Option<MemoryDetails>,
}

/// 창 위에 뜬 경고.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub message: String,
    /// `Go to <이름>` 단추: 가장 큰 터미널이 이 창에 있을 때만 있다.
    pub target: Option<(u32, String)>,
}

/// 자동 점검이 주입하는 가짜 값(`automation` 빌드에서만 쓴다).
#[cfg(feature = "automation")]
#[derive(Clone, Debug, Default)]
pub struct Injected {
    pub system: Option<(u64, u64)>,
    pub wsl: Option<WslMemory>,
}

/// 창이 쥐는 메모리 상태.
#[derive(Default)]
pub struct MemoryState {
    warned: bool,
    requested_at: Option<Instant>,
    n_seen_reports: u64,
    pub gauge: Option<Gauge>,
    pub notice: Option<Notice>,
    #[cfg(feature = "automation")]
    pub injected: Option<Injected>,
}

impl MemoryState {
    /// 데몬에 물을 때가 되었는지.
    pub fn due(
        &self,
        now: Instant,
    ) -> bool {
        self.requested_at.is_none_or(|at| now.duration_since(at) >= REQUEST_INTERVAL)
    }

    /// 요청을 보냈다고 적는다.
    pub fn mark_requested(
        &mut self,
        now: Instant,
    ) {
        self.requested_at = Some(now);
    }

    /// 새 답이 왔는지(왔으면 그 수를 적어 두고 true).
    pub fn take_new_report(&mut self) -> bool {
        let seen = n_reports();
        let is_new = seen != self.n_seen_reports;
        self.n_seen_reports = seen;
        is_new
    }

    /// 항목을 껐다: 게이지와 경고를 지우고 다음에 켜면 처음부터 시작한다.
    pub fn clear(&mut self) -> bool {
        let had = self.gauge.is_some() || self.notice.is_some();
        *self = Self::default();
        had
    }

    /// 경고를 닫는다.
    pub fn dismiss(&mut self) {
        self.notice = None;
    }

    /// 지금 값(이 컴퓨터는 직접, WSL은 최근 답)으로 게이지와 경고를 갱신한다. 바뀌었으면 true다.
    pub fn refresh(
        &mut self,
        name_in_window: &dyn Fn(u32) -> Option<String>,
    ) -> bool {
        #[cfg(feature = "automation")]
        let injected = self.injected.clone().map(|injected| (injected.system, injected.wsl));
        #[cfg(not(feature = "automation"))]
        let injected: Option<SystemAndWsl> = None;
        match injected {
            Some((system, wsl)) => self.refresh_with(system, wsl, name_in_window),
            None => self.refresh_with(system_memory(), latest_wsl(), name_in_window),
        }
    }

    /// 주어진 값으로 게이지와 경고를 갱신한다. 사용률이 처음 90%를 넘으면 경고를 한 번 띄우고, 80% 아래로 내려가야 다시 무장한다.
    pub fn refresh_with(
        &mut self,
        system: Option<(u64, u64)>,
        wsl: Option<WslMemory>,
        name_in_window: &dyn Fn(u32) -> Option<String>,
    ) -> bool {
        let (percent, source) = tightest_memory(system, wsl.as_ref());
        let details = percent.map(|percent| {
            memory_details(percent, source, wsl.as_ref(), system.map(|(total, _)| total), name_in_window)
        });
        let mut fired = false;
        if let (Some(percent), Some(details)) = (percent, &details) {
            let (fire, warned) = warn_step(percent, self.warned);
            self.warned = warned;
            if fire {
                self.notice = Some(Notice { message: details.warning.clone(), target: details.target.clone() });
                fired = true;
            }
        }
        // 화면에 늘 보이는 것은 사용률과 출처뿐이다. 말풍선 문장의 GB 값만 달라진 경우는 문장을 조용히 새 값으로 바꿔 두고
        // 다시 그리지 않는다(말풍선은 다음에 그릴 때 최신 문장으로 나온다). 경고가 새로 뜨면 다시 그린다.
        let visible = Some((percent, source));
        let changed = fired || self.gauge.as_ref().map(|gauge| (gauge.percent, gauge.source)) != visible;
        self.gauge = Some(Gauge { percent, source, details });
        changed
    }

    /// 경고가 이미 떴었는지(자동 점검 상태 글).
    pub fn warned(&self) -> bool {
        self.warned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn wsl_with(
        total: u64,
        available: u64,
        biggest: Option<(u32, u64)>,
    ) -> WslMemory {
        WslMemory {
            total,
            available,
            swap_total: 8 * GIB,
            terminals: biggest.into_iter().collect(),
            ..WslMemory::default()
        }
    }

    #[test]
    fn test_c_d76_memory_warns_once_at_90_and_rearms_below_80() {
        let mut state = MemoryState::default();
        let owner = |_: u32| None;
        let at = |state: &mut MemoryState, used: u64| state.refresh_with(Some((100, 100 - used)), None, &owner);
        at(&mut state, 50);
        assert!(state.notice.is_none() && !state.warned());
        at(&mut state, 92);
        assert!(state.notice.is_some() && state.warned(), "crossing 90 warns");
        state.dismiss();
        at(&mut state, 95);
        assert!(state.notice.is_none(), "still above 90 does not warn again");
        at(&mut state, 85);
        assert!(state.warned() && state.notice.is_none(), "between 80 and 90 stays warned");
        at(&mut state, 70);
        assert!(!state.warned());
        at(&mut state, 91);
        assert!(state.notice.is_some(), "after dropping below 80 the next crossing warns again");
    }

    #[test]
    fn test_c_d76_a_change_that_only_touches_the_tooltip_text_does_not_ask_for_a_redraw() {
        let mut state = MemoryState::default();
        let owner = |id: u32| (id == 4).then(|| "build".to_string());
        let at = |state: &mut MemoryState, wsl_available: u64, biggest: u64| {
            let wsl = wsl_with(26 * GIB, wsl_available, Some((4, biggest)));
            state.refresh_with(Some((32 * GIB, 16 * GIB)), Some(wsl), &owner)
        };
        assert!(at(&mut state, 13 * GIB, 3 * GIB), "the first reading is shown (50 % in WSL)");
        // 같은 사용률(50 %)에서 가장 큰 터미널의 크기만 달라졌다: 말풍선 문장은 새 값으로 바뀌지만 다시 그리지 않는다.
        let before = state.gauge.clone().and_then(|gauge| gauge.details).map(|details| details.summary);
        assert!(!at(&mut state, 13 * GIB, 4 * GIB));
        let after = state.gauge.clone().and_then(|gauge| gauge.details).map(|details| details.summary);
        assert_ne!(before, after, "the tooltip sentence follows the newest numbers");
        // 사용률이 달라지면 다시 그린다. 경고가 새로 뜨는 판독도 다시 그린다.
        assert!(at(&mut state, 5 * GIB, 4 * GIB));
        assert!(at(&mut state, 2 * GIB, 4 * GIB) && state.notice.is_some());
        assert!(!at(&mut state, 2 * GIB, 5 * GIB), "still above 90 and the same percent");
    }

    #[test]
    fn test_c_d76_the_largest_wsl_terminal_in_this_window_gets_a_go_to_button() {
        let mut state = MemoryState::default();
        let wsl = wsl_with(26 * GIB, 2 * GIB, Some((4, 3 * GIB)));
        state.refresh_with(Some((32 * GIB, 16 * GIB)), Some(wsl.clone()), &|id| (id == 4).then(|| "build".to_string()));
        let notice = state.notice.clone().expect("92% in WSL warns");
        assert_eq!(notice.target, Some((4, "Go to build".to_string())));
        assert!(notice.message.contains("build uses 3.0 GB"), "{}", notice.message);
        assert_eq!(state.gauge.as_ref().map(|gauge| (gauge.percent, gauge.source)), Some((Some(92), Source::Wsl)));
        let mut elsewhere = MemoryState::default();
        elsewhere.refresh_with(Some((32 * GIB, 16 * GIB)), Some(wsl), &|_| None);
        assert_eq!(
            elsewhere.notice.and_then(|notice| notice.target),
            None,
            "a terminal that is not in this window has no Go to button"
        );
    }

    #[test]
    fn test_c_d76_a_gauge_without_any_reading_has_no_percent_and_never_warns() {
        let mut state = MemoryState::default();
        state.refresh_with(None, None, &|_| None);
        assert_eq!(state.gauge.as_ref().map(|gauge| gauge.percent), Some(None));
        assert!(state.notice.is_none());
    }

    #[test]
    fn test_c_d76_reports_are_counted_and_stale_wsl_values_are_dropped() {
        let mut state = MemoryState::default();
        assert!(state.due(Instant::now()));
        state.mark_requested(Instant::now());
        assert!(!state.due(Instant::now()));
        let before = n_reports();
        store_report(MemoryReport { wsl: Some(wsl_with(10, 5, None)) });
        assert!(n_reports() > before);
        assert!(state.take_new_report());
        assert!(!state.take_new_report());
        assert_eq!(latest_wsl().map(|wsl| wsl.total), Some(10));
    }

    #[test]
    fn test_c_d76_meminfo_text_gives_total_and_available_bytes() {
        assert_eq!(
            parse_meminfo("MemTotal:       16384 kB\nMemFree: 1 kB\nMemAvailable:    8192 kB\n"),
            Some((16_777_216, 8_388_608))
        );
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
    }
}
