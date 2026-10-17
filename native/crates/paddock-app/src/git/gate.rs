//! git status를 언제 돌릴지 정하는 규칙(시각은 호출자가 넣는 순수 함수라 창 없이 시험한다).
//!
//! 놀 때(변경도 터미널 출력도 없을 때)는 아무것도 띄우지 않는 것이 핵심이다. 특히 WSL 폴더는 `wsl.exe`를 띄우는 값이 커서
//! 몇 가지 사건이 있을 때만 돈다. 사건을 받은 뒤에도 조용해지기를 잠깐 기다리고(디바운스), 직전 실행에서 최소 간격을 둔다.
use std::time::{Duration, Instant};

/// 사건을 받은 뒤 더 이어지는 사건이 없기를 기다리는 시간.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// 사건이 계속 이어져도 이 시간이 지나면 실행한다(끝없이 미루지 않는다).
pub const MAX_WAIT: Duration = Duration::from_secs(2);

/// 실행 시작 사이의 최소 간격. 변경이 쉬지 않아도 60초에 20번을 넘지 않는다.
pub const MIN_INTERVAL: Duration = Duration::from_secs(3);

/// status를 부를 만한 사건.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// 폴더 안 변경: 네이티브 폴더는 파일 감시 사건, WSL 폴더는 9P 수정 시각 비교가 본 변경.
    Change,
    /// 이 저장소 폴더 안 터미널의 출력. WSL 폴더에서만 뜻이 있다(네이티브는 파일 감시가 변경을 본다).
    Output,
    /// 이 폴더를 처음 보이게 되었다.
    Shown,
    /// Source control 화면을 열었거나 창이 활성으로 돌아왔다. WSL 폴더에서만 뜻이 있다.
    Entered,
    /// 스테이징·커밋·되돌리기를 마쳤다. 사용자가 바로 결과를 기다리므로 간격을 두지 않는다.
    Written,
}

/// 한 폴더의 status 실행 시점 판정.
#[derive(Debug)]
pub struct Gate {
    wsl: bool,
    /// 디바운스를 거치는 사건(`Change`·`Output`)을 처음/마지막으로 받은 시각.
    quiet_first: Option<Instant>,
    quiet_last: Option<Instant>,
    /// 기다리지 않아도 되는 사건(`Shown`·`Entered`)이 있다.
    ready: bool,
    /// 간격 없이 돌려야 하는 사건(`Written`)이 있다.
    written: bool,
    last_started: Option<Instant>,
    running: bool,
}

impl Gate {
    /// `wsl`은 폴더가 WSL 배포판 안인지다.
    pub fn new(wsl: bool) -> Self {
        Self {
            wsl,
            quiet_first: None,
            quiet_last: None,
            ready: false,
            written: false,
            last_started: None,
            running: false,
        }
    }

    /// 사건을 받았다.
    pub fn request(
        &mut self,
        trigger: Trigger,
        now: Instant,
    ) {
        match trigger {
            // 네이티브 폴더는 파일 감시가 변경을 보므로 터미널 출력·창 활성 복귀는 사건이 아니다.
            Trigger::Output | Trigger::Entered if !self.wsl => {}
            Trigger::Change | Trigger::Output => {
                self.quiet_first.get_or_insert(now);
                self.quiet_last = Some(now);
            }
            Trigger::Shown | Trigger::Entered => self.ready = true,
            Trigger::Written => self.written = true,
        }
    }

    /// 실행을 기다리는 사건이 있다.
    pub fn is_pending(&self) -> bool {
        self.quiet_first.is_some() || self.ready || self.written
    }

    /// 지금 실행해도 되는지. 실행 중이면 끝난 뒤에 다시 본다.
    pub fn is_due(
        &self,
        now: Instant,
    ) -> bool {
        let spaced = self.last_started.is_none_or(|started| now >= started + MIN_INTERVAL);
        let settled = match (self.quiet_first, self.quiet_last) {
            (Some(first), Some(last)) => now >= (last + DEBOUNCE).min(first + MAX_WAIT),
            _ => false,
        };
        !self.running && (self.written || (spaced && (self.ready || settled)))
    }

    /// 실행을 시작했다. 기다리던 사건은 이 실행이 맡는다.
    pub fn started(
        &mut self,
        now: Instant,
    ) {
        self.quiet_first = None;
        self.quiet_last = None;
        self.ready = false;
        self.written = false;
        self.last_started = Some(now);
        self.running = true;
    }

    /// 실행이 끝났다.
    pub fn finished(&mut self) {
        self.running = false;
    }

    /// 실행 중이다.
    pub fn is_running(&self) -> bool {
        self.running
    }
}

/// 터미널 출력 사건을 가려내는 기억: 터미널마다 마지막으로 본 출력 시각.
///
/// 처음 보는 출력(시작·되살리기 재생)은 기준만 잡고 사건으로 세지 않는다. 닫힌 탭의 기억은 버리고, 데몬이 바뀌어 같은 번호가 다른 터미널에
/// 다시 매겨지면 새 터미널로 본다(번호만으로 묶으면 옛 터미널의 기억이 새 터미널의 첫 출력을 사건으로 만든다).
#[derive(Debug, Default)]
pub struct OutputWatch {
    seen: std::collections::HashMap<(String, u32), Instant>,
}

impl OutputWatch {
    /// 지금 있는 터미널들(`daemon`, 번호, 마지막 출력 시각)을 훑어 새 출력을 본 터미널의 번호를 돌려준다.
    pub fn observe<'a>(
        &mut self,
        tabs: impl IntoIterator<Item = (&'a str, u32, Option<Instant>)>,
    ) -> Vec<u32> {
        let mut changed = Vec::new();
        let mut alive = std::collections::HashSet::new();
        for (daemon, id, latest) in tabs {
            let key = (daemon.to_string(), id);
            alive.insert(key.clone());
            if let Some(latest) = latest {
                let known = self.seen.insert(key, latest);
                if known.is_some_and(|known| known != latest) {
                    changed.push(id);
                }
            }
        }
        self.seen.retain(|key, _| alive.contains(key));
        changed
    }

    /// 기억하는 터미널 수.
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_review_12_the_window_coming_back_only_opens_the_gate_of_a_wsl_folder() {
        let mut native = Gate::new(false);
        native.request(Trigger::Entered, at(0));
        assert!(!native.is_pending(), "the watcher already sees changes of a native folder");
        let mut wsl = Gate::new(true);
        wsl.request(Trigger::Entered, at(0));
        assert!(wsl.is_due(at(0)));
    }

    #[test]
    fn test_c_review_6_output_memory_drops_closed_tabs_and_treats_a_reused_number_of_a_new_daemon_as_new() {
        let mut watch = OutputWatch::default();
        let (t0, t1) = (at(0), at(100));
        assert!(watch.observe([("d1", 1, Some(t0)), ("d1", 2, None)]).is_empty(), "first sight is only a baseline");
        assert_eq!(watch.observe([("d1", 1, Some(t1))]), [1], "a later output is an event");
        assert_eq!(watch.len(), 1, "the closed tab 2 is forgotten");
        // 데몬이 바뀌어 같은 번호 1이 다른 터미널이 되었다: 첫 출력은 기준일 뿐이다.
        assert!(watch.observe([("d2", 1, Some(t0))]).is_empty());
        assert_eq!(watch.len(), 1, "the old daemon's memory is gone");
        assert!(watch.observe([("d2", 1, Some(t0))]).is_empty(), "no new output, no event");
    }

    fn at(ms: u64) -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn test_c_d52_an_idle_gate_never_runs() {
        for wsl in [false, true] {
            let gate = Gate::new(wsl);
            assert!((0..120_000).step_by(100).all(|ms| !gate.is_due(at(ms))), "wsl={wsl}");
        }
    }

    #[test]
    fn test_c_d52_terminal_output_is_ignored_for_a_native_folder_and_counts_for_a_wsl_folder() {
        let mut native = Gate::new(false);
        native.request(Trigger::Output, at(0));
        assert!(!native.is_pending() && !native.is_due(at(10_000)));
        let mut wsl = Gate::new(true);
        wsl.request(Trigger::Output, at(0));
        assert!(!wsl.is_due(at(400)), "it waits for the output to settle");
        assert!(wsl.is_due(at(500)));
    }

    #[test]
    fn test_c_d52_gate_merges_the_events_of_one_debounce_window_into_one_run() {
        let mut gate = Gate::new(false);
        for ms in [0, 100, 200] {
            gate.request(Trigger::Change, at(ms));
        }
        assert!(!gate.is_due(at(600)), "the last event was 400 ms ago");
        assert!(gate.is_due(at(700)));
        gate.started(at(700));
        assert!(!gate.is_pending(), "the three events were one run");
        gate.finished();
        assert!(!gate.is_due(at(5_000)));
    }

    #[test]
    fn test_c_d52_events_that_never_stop_still_run_after_the_longest_wait() {
        let mut gate = Gate::new(true);
        for ms in (0..2_000).step_by(100) {
            gate.request(Trigger::Output, at(ms));
        }
        assert!(gate.is_due(at(2_000)));
    }

    #[test]
    fn test_c_d52_runs_start_at_least_three_seconds_apart_but_a_write_does_not_wait() {
        let mut gate = Gate::new(true);
        gate.request(Trigger::Shown, at(0));
        assert!(gate.is_due(at(0)));
        gate.started(at(0));
        gate.finished();
        gate.request(Trigger::Entered, at(1_000));
        assert!(!gate.is_due(at(1_000)) && !gate.is_due(at(2_999)));
        assert!(gate.is_due(at(3_000)));
        gate.request(Trigger::Written, at(1_100));
        assert!(gate.is_due(at(1_100)), "after staging or committing the result is wanted at once");
    }

    #[test]
    fn test_c_d52_an_event_during_a_run_waits_for_the_run_to_finish() {
        let mut gate = Gate::new(false);
        gate.request(Trigger::Written, at(0));
        gate.started(at(0));
        gate.request(Trigger::Written, at(50));
        assert!(!gate.is_due(at(10_000)), "never two runs at once");
        gate.finished();
        assert!(gate.is_due(at(10_000)));
    }

    #[test]
    fn test_c_d52_changes_every_100_ms_for_a_minute_run_at_most_21_times() {
        let mut gate = Gate::new(true);
        let mut n_runs = 0;
        for ms in (0..60_000).step_by(100) {
            gate.request(Trigger::Change, at(ms));
            if gate.is_due(at(ms)) {
                n_runs += 1;
                gate.started(at(ms));
                gate.finished();
            }
        }
        assert!(n_runs <= 21, "{n_runs}");
        assert!(n_runs >= 15, "it still keeps up: {n_runs}");
    }
}
