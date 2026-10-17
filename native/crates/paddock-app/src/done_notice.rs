//! 에이전트 완료 알림의 앱 쪽 규칙: 어느 터미널에 미확인 표시를 붙이고 소리를 낼지.
//!
//! 지금 보는 터미널(선택됨·화면에 보임·창 활성)이 끝나면 소리도 표시도 없다 — 사용자가 이미 보고 있다.
//! 다른 터미널이 끝나면 미확인 표시를 붙이고, 마지막 소리 뒤 쿨다운이 지났을 때만 소리를 낸다.
//! 쿨다운은 소리만 거른다: 여러 에이전트가 거의 동시에 끝나도 어느 터미널이 끝났는지는 표시로 알 수 있다.
//! 표시는 그 터미널을 실제로 확인할 때만 지운다.
use std::collections::BTreeSet;

use paddock_core::agent_model::should_sound;

/// 완료 알림 하나를 처리한 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 완료 소리를 내야 한다.
    pub sound: bool,
}

/// 미확인 표시가 붙은 터미널과 마지막 소리 시각.
#[derive(Debug, Default)]
pub struct DoneNotice {
    unseen: BTreeSet<u32>,
    last_sound_at: Option<u64>,
}

impl DoneNotice {
    /// 터미널 `id`의 완료 알림을 처리한다. `watching`이면 아무것도 붙이지 않는다.
    /// `sound_enabled`가 거짓이면 표시만 붙이고 쿨다운도 시작하지 않는다. `now`는 밀리초 시각이다.
    pub fn on_finished(
        &mut self,
        id: u32,
        watching: bool,
        sound_enabled: bool,
        now: u64,
    ) -> Outcome {
        let mut sound = false;
        if !watching {
            self.unseen.insert(id);
            sound = sound_enabled && should_sound(self.last_sound_at, now);
            if sound {
                self.last_sound_at = Some(now);
            }
        }
        Outcome { sound }
    }

    /// 터미널을 확인했다. 표시가 있었으면 지우고 참을 돌려준다.
    pub fn acknowledge(
        &mut self,
        id: u32,
    ) -> bool {
        self.unseen.remove(&id)
    }

    /// 닫힌 터미널의 표시를 지운다.
    pub fn forget(
        &mut self,
        id: u32,
    ) {
        self.unseen.remove(&id);
    }

    pub fn is_unseen(
        &self,
        id: u32,
    ) -> bool {
        self.unseen.contains(&id)
    }

    /// 표시가 붙은 터미널 번호(오름차순).
    pub fn unseen_ids(&self) -> Vec<u32> {
        self.unseen.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_core::agent_model::limit::SOUND_COOLDOWN_MS;

    #[test]
    fn test_c_d37_watched_terminal_is_acknowledged_without_sound() {
        let mut notice = DoneNotice::default();
        let outcome = notice.on_finished(3, true, true, 1000);
        assert_eq!(outcome, Outcome { sound: false });
        assert!(!notice.is_unseen(3), "the user is looking at it: nothing to mark");
    }

    #[test]
    fn test_c_d37_an_unwatched_finish_marks_the_terminal_and_plays_the_sound() {
        let mut notice = DoneNotice::default();
        assert_eq!(notice.on_finished(3, false, true, 1000), Outcome { sound: true });
        assert!(notice.is_unseen(3));
        assert_eq!(notice.unseen_ids(), vec![3]);
    }

    #[test]
    fn test_c_d37_sound_is_skipped_inside_cooldown() {
        let mut notice = DoneNotice::default();
        assert!(notice.on_finished(1, false, true, 1000).sound);
        // 거의 동시에 끝난 다른 에이전트: 소리는 거르고 표시는 붙는다.
        assert!(!notice.on_finished(2, false, true, 1000 + SOUND_COOLDOWN_MS - 1).sound);
        assert!(notice.is_unseen(2));
        assert!(notice.on_finished(2, false, true, 1000 + SOUND_COOLDOWN_MS).sound);
    }

    #[test]
    fn test_a_skipped_sound_does_not_restart_the_cooldown() {
        let mut notice = DoneNotice::default();
        assert!(notice.on_finished(1, false, true, 0).sound);
        assert!(!notice.on_finished(2, false, true, SOUND_COOLDOWN_MS - 1).sound);
        assert!(
            notice.on_finished(3, false, true, SOUND_COOLDOWN_MS).sound,
            "counted from the last sound, not the last finish"
        );
    }

    #[test]
    fn test_a_disabled_sound_setting_marks_without_sound_or_cooldown() {
        let mut notice = DoneNotice::default();
        assert_eq!(notice.on_finished(1, false, false, 1000), Outcome { sound: false });
        assert!(notice.is_unseen(1));
        assert!(notice.on_finished(2, false, true, 1001).sound, "an earlier muted finish does not start the cooldown");
    }

    #[test]
    fn test_selecting_or_closing_clears_the_mark() {
        let mut notice = DoneNotice::default();
        notice.on_finished(1, false, true, 0);
        notice.on_finished(2, false, true, 10_000);
        assert!(notice.acknowledge(1));
        assert!(!notice.acknowledge(1), "nothing left to clear");
        assert!(!notice.is_unseen(1));
        notice.forget(2);
        assert!(notice.unseen_ids().is_empty());
    }

    #[test]
    fn test_the_mark_stays_when_output_resumes_and_only_a_real_look_clears_it() {
        let mut notice = DoneNotice::default();
        notice.on_finished(4, false, true, 0);
        // 같은 터미널이 또 끝나도 표시는 하나다.
        notice.on_finished(4, false, true, 20_000);
        assert_eq!(notice.unseen_ids(), vec![4]);
    }
}
