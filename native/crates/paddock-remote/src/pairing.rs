//! 기기 승인 창: 맥에서 연 5분짜리 1회용 코드로만 새 기기를 페어링한다.
//!
//! 창은 메모리에만 있고 코드는 디스크에 닿지 않는다. 코드는 128비트 무작위 값이고, 한 번 맞으면 창이 닫히며,
//! 5번 틀리면 맞는 코드를 넣어도 이미 닫혀 있다. 시각은 호출자가 넘기는 `Instant`라 시험이 기다리지 않는다.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rand::RngCore;

use crate::devices::constant_time_eq;

/// 창의 수명과 실패 허용 횟수.
pub struct PairingRule;

impl PairingRule {
    pub const WINDOW: Duration = Duration::from_secs(5 * 60);
    pub const MAX_FAILURES: u32 = 5;
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

/// 기기 승인 창(없거나 하나).
#[derive(Default)]
pub struct Pairing {
    window: Mutex<Option<Window>>,
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
        *self.lock() = Some(Window { code: code.clone(), opened: now, n_failures: 0 });
        code
    }

    /// 코드를 낸다.
    ///
    /// Examples
    /// --------
    /// | 창의 상태                  | 낸 코드 | 결과                                  |
    /// |----------------------------|---------|---------------------------------------|
    /// | 열림, 5분 안               | 맞음    | `Accepted`, 창이 닫힌다(1회용)        |
    /// | 열림, 5분 안               | 틀림    | `Wrong`, 5번째 실패면 `closed: true`  |
    /// | 5분이 지남                 | 무엇이든 | `NotOpen`                            |
    /// | 닫힘·열린 적 없음          | 무엇이든 | `NotOpen`                            |
    pub fn submit(
        &self,
        given: &str,
        now: Instant,
    ) -> Submission {
        let mut guard = self.lock();
        if guard.as_ref().is_some_and(|window| window.is_expired(now)) {
            *guard = None;
        }
        let outcome = match guard.as_mut() {
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
        if matches!(outcome, Submission::Accepted | Submission::Wrong { closed: true, .. }) {
            *guard = None;
        }
        outcome
    }

    /// 창이 열려 있으면 닫히기까지 남은 시간.
    pub fn remaining(
        &self,
        now: Instant,
    ) -> Option<Duration> {
        let mut guard = self.lock();
        if guard.as_ref().is_some_and(|window| window.is_expired(now)) {
            *guard = None;
        }
        guard.as_ref().map(|window| PairingRule::WINDOW.saturating_sub(now.saturating_duration_since(window.opened)))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Window>> {
        self.window.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
    }

    #[test]
    fn test_window_expires_after_five_minutes() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let code = pairing.open(now);
        let almost = now + PairingRule::WINDOW - Duration::from_secs(1);
        assert_eq!(pairing.remaining(almost), Some(Duration::from_secs(1)));
        assert_eq!(pairing.submit(&code, now + PairingRule::WINDOW), Submission::NotOpen);
        assert_eq!(pairing.remaining(now + PairingRule::WINDOW), None);
    }

    #[test]
    fn test_window_closes_after_five_failures_even_for_the_right_code() {
        let pairing = Pairing::default();
        let now = Instant::now();
        let code = pairing.open(now);
        for n_failures in 1..PairingRule::MAX_FAILURES {
            assert_eq!(pairing.submit("wrong", now), Submission::Wrong { n_failures, closed: false });
        }
        assert_eq!(
            pairing.submit("wrong", now),
            Submission::Wrong { n_failures: PairingRule::MAX_FAILURES, closed: true }
        );
        assert_eq!(pairing.submit(&code, now), Submission::NotOpen);
    }

    #[test]
    fn test_never_opened_window_accepts_nothing() {
        let pairing = Pairing::default();
        assert_eq!(pairing.submit("", Instant::now()), Submission::NotOpen);
    }
}
