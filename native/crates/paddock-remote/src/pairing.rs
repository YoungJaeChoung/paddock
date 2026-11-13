//! 기기 승인 창: 맥에서 연 5분짜리 1회용 코드로만 새 기기를 페어링한다.
//!
//! 창은 메모리에만 있고 코드는 디스크에 닿지 않는다. 코드는 128비트 무작위 값이라 추측으로는 맞힐 수 없고, 한 번 맞으면 창이 닫힌다.
//! 틀린 코드가 `MAX_FAILURES`번 쌓이면 창이 닫혀 무차별 시도를 끊는다. 상한을 크게 잡은 것은 아무나 몇 번 틀려 창을 닫아 버리는 방해를 어렵게 하려는 것이고,
//! 닫힌 이유는 `last_closed`로 남아 앱이 창을 다시 열도록 안내할 수 있다. 시각은 호출자가 넘기는 `Instant`라 시험이 기다리지 않는다.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngCore;

use crate::devices::constant_time_eq;

/// 창의 수명과 실패 허용 횟수.
pub struct PairingRule;

impl PairingRule {
    pub const WINDOW: Duration = Duration::from_secs(5 * 60);
    pub const MAX_FAILURES: u32 = 100;
}

/// 열린 창 하나.
struct Window {
    code: String,
    opened: Instant,
    n_failures: u32,
}

impl Window {
    fn is_expired(
        &self,
        now: Instant,
    ) -> bool {
        now.saturating_duration_since(self.opened) >= PairingRule::WINDOW
    }
}

/// 창이 닫힌 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// 코드가 맞아 기기가 페어링됐다.
    Accepted,
    TooManyFailures,
    Expired,
}

impl CloseReason {
    /// 상태 응답과 기록에 쓰는 이름.
    pub fn name(self) -> &'static str {
        match self {
            CloseReason::Accepted => "accepted",
            CloseReason::TooManyFailures => "too-many-failures",
            CloseReason::Expired => "expired",
        }
    }
}

/// 코드를 내본 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submission {
    /// 코드가 맞았다. 창은 닫혔다.
    Accepted,
    /// 코드가 틀렸다. `n_failures`는 이 창의 누적 실패 수이고, `closed`이면 이 실패로 창이 닫혔다.
    Wrong { n_failures: u32, closed: bool },
    /// 열린 창이 없다(열지 않았거나, 시간이 지났거나, 이미 닫혔다).
    NotOpen,
}

/// 창과 마지막으로 닫힌 이유.
#[derive(Default)]
struct State {
    window: Option<Window>,
    last_closed: Option<CloseReason>,
}

impl State {
    /// 시간이 지난 창을 닫는다.
    fn expire(
        &mut self,
        now: Instant,
    ) {
        if self.window.as_ref().is_some_and(|window| window.is_expired(now)) {
            self.window = None;
            self.last_closed = Some(CloseReason::Expired);
        }
    }
}

/// 기기 승인 창(없거나 하나).
#[derive(Default)]
pub struct Pairing {
    state: Mutex<State>,
}

impl Pairing {
    /// 새 창을 열고 코드(16진수 32자)를 돌려준다. 열려 있던 창은 닫히고 그 코드는 못 쓴다.
    pub fn open(
        &self,
        now: Instant,
    ) -> String {
        let mut bytes = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let code: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut state = self.lock();
        state.window = Some(Window { code: code.clone(), opened: now, n_failures: 0 });
        state.last_closed = None;
        code
    }

    /// 코드를 낸다.
    ///
    /// Examples
    /// --------
    /// | 창의 상태                  | 낸 코드  | 결과                                           |
    /// |----------------------------|----------|------------------------------------------------|
    /// | 열림, 5분 안               | 맞음     | `Accepted`, 창이 닫힌다(1회용)                 |
    /// | 열림, 5분 안               | 틀림     | `Wrong`, `MAX_FAILURES`번째 실패면 `closed: true` |
    /// | 5분이 지남                 | 무엇이든 | `NotOpen`                                      |
    /// | 닫힘·열린 적 없음          | 무엇이든 | `NotOpen`                                      |
    pub fn submit(
        &self,
        given: &str,
        now: Instant,
    ) -> Submission {
        let mut state = self.lock();
        state.expire(now);
        let outcome = match state.window.as_mut() {
            None => Submission::NotOpen,
            Some(window) if constant_time_eq(given.as_bytes(), window.code.as_bytes()) => Submission::Accepted,
            Some(window) => {
                window.n_failures += 1;
                Submission::Wrong {
                    n_failures: window.n_failures,
                    closed: window.n_failures >= PairingRule::MAX_FAILURES,
                }
            }
        };
        match outcome {
            Submission::Accepted => {
                state.window = None;
                state.last_closed = Some(CloseReason::Accepted);
            }
            Submission::Wrong { closed: true, .. } => {
                state.window = None;
                state.last_closed = Some(CloseReason::TooManyFailures);
            }
            _ => {}
        }
        outcome
    }

    /// 창이 열려 있으면 닫히기까지 남은 시간.
    pub fn remaining(
        &self,
        now: Instant,
    ) -> Option<Duration> {
        let mut state = self.lock();
        state.expire(now);
        state
            .window
            .as_ref()
            .map(|window| PairingRule::WINDOW.saturating_sub(now.saturating_duration_since(window.opened)))
    }

    /// 가장 최근 창이 닫힌 이유. 창이 열려 있거나 연 적이 없으면 `None`이다.
    pub fn last_closed(
        &self,
        now: Instant,
    ) -> Option<CloseReason> {
        let mut state = self.lock();
        state.expire(now);
        state.last_closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_is_128_bit_hex_and_differs_per_window() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let first = pairing.open(now);
        let second = pairing.open(now);
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
        assert_eq!(
            pairing.submit(&first, now),
            Submission::Wrong { n_failures: 1, closed: false },
            "replaced window's code is dead"
        );
    }

    #[test]
    fn test_code_works_once() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let code = pairing.open(now);
        assert_eq!(pairing.submit(&code, now), Submission::Accepted);
        assert_eq!(pairing.submit(&code, now), Submission::NotOpen, "single use");
        assert_eq!(pairing.remaining(now), None);
        assert_eq!(pairing.last_closed(now), Some(CloseReason::Accepted));
    }

    #[test]
    fn test_window_expires_after_five_minutes() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let code = pairing.open(now);
        let almost = now + PairingRule::WINDOW - Duration::from_secs(1);
        assert_eq!(pairing.remaining(almost), Some(Duration::from_secs(1)));
        assert_eq!(pairing.last_closed(almost), None);
        assert_eq!(pairing.submit(&code, now + PairingRule::WINDOW), Submission::NotOpen);
        assert_eq!(pairing.remaining(now + PairingRule::WINDOW), None);
        assert_eq!(pairing.last_closed(now + PairingRule::WINDOW), Some(CloseReason::Expired));
    }

    #[test]
    fn test_window_closes_after_the_failure_cap_even_for_the_right_code() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let code = pairing.open(now);
        for n_failures in 1..PairingRule::MAX_FAILURES {
            assert_eq!(pairing.submit("wrong", now), Submission::Wrong { n_failures, closed: false });
        }
        assert_eq!(pairing.last_closed(now), None, "still open below the cap");
        assert_eq!(
            pairing.submit("wrong", now),
            Submission::Wrong { n_failures: PairingRule::MAX_FAILURES, closed: true }
        );
        assert_eq!(pairing.submit(&code, now), Submission::NotOpen);
        assert_eq!(pairing.last_closed(now), Some(CloseReason::TooManyFailures));
        // 다시 열면 이유가 지워지고 실패 수도 0에서 시작한다.
        let again = pairing.open(now);
        assert_eq!(pairing.last_closed(now), None);
        assert_eq!(pairing.submit(&again, now), Submission::Accepted);
    }

    #[test]
    fn test_never_opened_window_accepts_nothing() {
        let pairing = Pairing::default();
        assert_eq!(pairing.submit("", Instant::now()), Submission::NotOpen);
        assert_eq!(pairing.last_closed(Instant::now()), None);
    }
}
