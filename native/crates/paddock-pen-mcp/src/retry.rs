//! 저장 재시도 규칙: 저장을 부르기 전에 난 일시적 실패만 다시 시도한다.
use std::time::Duration;

/// 저장 시도의 실패. `Retryable`은 잠깐 뒤 풀릴 수 있는 실패다.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveError {
    /// 창을 못 찾음·메뉴가 안 열림·활성 문서가 바뀜·Save 항목이 아직 없음. 모두 저장을 부르기 전에만 나므로 다시 시도해도 두 번 저장되지 않는다.
    Retryable(String),
    /// 다시 시도해도 같은 결과이거나 다시 시도하면 안 되는 실패(창이 여럿·Save 항목이 둘 이상·저장 안 됨·다른 파일이 바뀜·열기 실패).
    /// 저장을 이미 불렀을 수 있어 다시 시도하면 안 된다.
    Fatal(String),
}

/// `operation`을 성공하거나 재시도할 수 없는 실패가 날 때까지, 최대 `max_attempts`번 부른다. 재시도 사이에 `delay`만큼 쉰다.
///
/// 마지막 시도가 `Retryable`로 실패해도 그 문장을 오류로 돌려준다.
///
/// Examples
/// --------
/// | 시도 결과 순서                           | max_attempts | 결과                |
/// |------------------------------------------|--------------|---------------------|
/// | Ok                                       | 4            | Ok, 1번 부름        |
/// | Retryable, Retryable, Ok                 | 4            | Ok, 3번 부름        |
/// | Retryable, Fatal                         | 4            | Err(Fatal 문장), 2번 |
/// | Retryable ×4                             | 4            | Err(마지막 문장), 4번 |
pub fn with_retries<T>(
    max_attempts: usize,
    delay: Duration,
    mut operation: impl FnMut() -> Result<T, SaveError>,
) -> Result<T, String> {
    let mut attempt = 1;
    loop {
        match operation() {
            Ok(value) => break Ok(value),
            Err(SaveError::Retryable(message)) if attempt >= max_attempts => break Err(message),
            Err(SaveError::Retryable(_)) => {
                attempt += 1;
                std::thread::sleep(delay);
            }
            Err(SaveError::Fatal(message)) => break Err(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(results: Vec<Result<u32, SaveError>>) -> (Result<u32, String>, usize) {
        let mut queue = results.into_iter();
        let mut calls = 0;
        let outcome = with_retries(4, Duration::ZERO, || {
            calls += 1;
            queue.next().expect("called more often than expected")
        });
        (outcome, calls)
    }

    fn retryable(text: &str) -> Result<u32, SaveError> {
        Err(SaveError::Retryable(text.to_string()))
    }

    #[test]
    fn test_retry_1_success_stops_at_once_and_a_retryable_failure_is_tried_again() {
        assert_eq!(run(vec![Ok(1)]), (Ok(1), 1));
        assert_eq!(run(vec![retryable("a"), retryable("b"), Ok(3)]), (Ok(3), 3));
    }

    #[test]
    fn test_retry_2_a_fatal_failure_is_never_tried_again() {
        let (outcome, calls) = run(vec![retryable("a"), Err(SaveError::Fatal("not saved".into()))]);
        assert_eq!((outcome, calls), (Err("not saved".to_string()), 2));
    }

    #[test]
    fn test_retry_3_the_last_retryable_message_is_returned_after_the_attempts_run_out() {
        let (outcome, calls) = run(vec![retryable("1"), retryable("2"), retryable("3"), retryable("4")]);
        assert_eq!((outcome, calls), (Err("4".to_string()), 4));
    }
}
