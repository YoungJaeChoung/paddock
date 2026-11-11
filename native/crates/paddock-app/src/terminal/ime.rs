//! 터미널 한글 조합(IME) 상태기계: 조합 중 글자는 PTY에 보내지 않고 들고 있다가, 확정될 때 한 번만 보낼 바이트로 돌려준다.
//!
//! 입력 방식(IME)이 주는 조합 갱신·확정과 키 입력이 어떤 순서로 와도 PTY에는 확정 글자 → 키 순서로 나가고,
//! 같은 글자가 두 번 나가지 않게 하는 규칙만 담는다. 창·그리기 도구를 모르는 순수 로직이라 값으로 검증한다.
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// 입력 방식이 뒤늦게 알릴 수 있는 확정 글자의 기억을 유지하는 시간. 이 안에 같은 글자가 확정으로 오면 한 번 버린다.
const LATE_COMMIT_WINDOW: Duration = Duration::from_millis(500);

/// 키·포커스 이동으로 이미 내보낸 조합 글자의 기억. 같은 창의 모든 터미널 보기가 함께 쥔다.
///
/// 탭을 바꾸면 이전 탭이 조합 글자를 내보낸 뒤, 입력 방식이 같은 글자의 확정을 새로 포커스를 받은 다른 탭에 알릴 수 있다.
/// 기억이 탭마다 따로면 그 뒤늦은 확정이 새 탭에 한 번 더 나가므로 창 하나에 하나를 둔다.
/// 뒤늦은 알림은 곧바로 오므로 기억은 `LATE_COMMIT_WINDOW` 안에서만 쓰고, 한 번 쓰면 지운다.
#[derive(Clone, Debug, Default)]
pub struct SentComposition(Rc<RefCell<Option<(String, Instant)>>>);

impl SentComposition {
    fn remember(
        &self,
        text: String,
        now: Instant,
    ) {
        *self.0.borrow_mut() = Some((text, now));
    }

    /// 터미널이 아닌 입력칸(이름 바꾸기 등)이 `value`의 마지막 글자를 확정으로 받았다.
    ///
    /// 입력 방식이 같은 글자의 확정을 입력칸이 닫힌 뒤 터미널에 뒤늦게 알릴 수 있으므로, 키·포커스 이동으로 내보낸 글자와
    /// 같게 기억해 그 확정을 한 번 버린다. 뒤늦은 알림은 조합으로 확정된 글자(ASCII가 아닌 글자)에서만 오므로
    /// ASCII로 끝나는 값은 기억하지 않는다 — 이름이 `build`로 끝나도 터미널에 처음 친 `d`는 버려지지 않는다. 빈 값은 기억할 글자가 없다.
    pub fn note_commit_elsewhere(
        &self,
        value: &str,
    ) {
        self.note_commit_elsewhere_at(value, Instant::now());
    }

    fn note_commit_elsewhere_at(
        &self,
        value: &str,
        now: Instant,
    ) {
        if let Some(last) = value.chars().last().filter(|last| !last.is_ascii()) {
            self.remember(last.to_string(), now);
        }
    }

    fn forget(&self) {
        self.0.borrow_mut().take();
    }

    /// 방금 내보낸 글자와 같은 확정이면 기억을 지우고 true다(그 확정은 버려야 한다).
    fn is_duplicate_of(
        &self,
        text: &str,
    ) -> bool {
        self.is_duplicate_of_at(text, Instant::now())
    }

    fn is_duplicate_of_at(
        &self,
        text: &str,
        now: Instant,
    ) -> bool {
        self.0
            .borrow_mut()
            .take()
            .is_some_and(|(sent, at)| sent == text && now.saturating_duration_since(at) <= LATE_COMMIT_WINDOW)
    }
}

/// 조합 중인 글자와 커서, 키로 이미 내보낸 글자의 뒤늦은 확정 알림을 버리기 위한 기억.
#[derive(Debug, Default)]
pub struct ImeState {
    /// 조합 중인 글자. 비어 있으면 조합 중이 아니다.
    marked: String,
    /// 조합 글자 안의 커서 위치(UTF-16 단위). 후보창을 그 칸 옆에 띄우는 데 쓴다.
    caret_utf16: usize,
    /// 키·포커스 이동으로 이미 내보낸 조합 글자. 입력 방식이 같은 글자를 뒤늦게 확정으로 알리면 한 번 버린다.
    already_sent: SentComposition,
}

/// 키 하나를 처리하기 전에 내보낼 바이트와, 키 자체를 PTY에 보낼지.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyPlan {
    /// 키보다 먼저 보낼 조합 글자(확정). 조합 중이 아니면 비어 있다.
    pub flushed: Vec<u8>,
    /// 키 자체의 바이트를 이어서 보낼지.
    pub send_key: bool,
}

impl ImeState {
    /// 조합 상태를 만든다. `already_sent`는 같은 창의 다른 보기와 함께 쓰는 기억이다.
    pub fn new(already_sent: SentComposition) -> Self {
        Self { already_sent, ..Self::default() }
    }

    /// 조합 중인 글자. 조합 중이 아니면 None이다.
    pub fn marked(&self) -> Option<&str> {
        if self.marked.is_empty() { None } else { Some(self.marked.as_str()) }
    }

    /// 조합 글자 안의 커서 위치(UTF-16 단위).
    pub fn caret_utf16(&self) -> usize {
        self.caret_utf16
    }

    /// 조합 글자를 바꾼다. 빈 글자는 조합이 끝났다는 뜻이다. `caret`이 없으면 글자 끝이다.
    pub fn mark(
        &mut self,
        text: &str,
        caret: Option<Range<usize>>,
    ) {
        if !text.is_empty() {
            self.already_sent.forget();
        }
        let n_units = text.encode_utf16().count();
        self.marked = text.to_string();
        self.caret_utf16 = caret.map_or(n_units, |range| range.end.min(n_units));
    }

    /// 입력 방식이 글자를 확정했다. PTY로 보낼 바이트를 돌려준다.
    pub fn commit(
        &mut self,
        text: &str,
    ) -> Vec<u8> {
        let duplicate = self.already_sent.is_duplicate_of(text);
        self.marked.clear();
        self.caret_utf16 = 0;
        if duplicate { Vec::new() } else { text.as_bytes().to_vec() }
    }

    /// 키 이름(`enter`, `escape`, `backspace` …)을 처리하기 전에 할 일을 정한다. `would_send`는 이 키가 PTY로 보낼 바이트가 있는지다.
    pub fn plan_key(
        &mut self,
        key: &str,
        would_send: bool,
    ) -> KeyPlan {
        // 이전 키·포커스 이동으로 기다리던 뒤늦은 확정 알림은 이 키가 오면 더 오지 않는다.
        self.already_sent.forget();
        let hands_off = !would_send || (key == "backspace" && !self.marked.is_empty());
        let flushed = if hands_off { Vec::new() } else { self.flush() };
        KeyPlan { flushed, send_key: !hands_off }
    }

    /// 조합 글자를 지금 확정으로 내보낸다(포커스가 떠났을 때). 입력 방식이 같은 글자를 뒤늦게 알리면 버리도록 기억한다.
    pub fn flush(&mut self) -> Vec<u8> {
        let text = std::mem::take(&mut self.marked);
        self.caret_utf16 = 0;
        let bytes = text.as_bytes().to_vec();
        if !text.is_empty() {
            self.already_sent.remember(text, Instant::now());
        }
        bytes
    }
}

/// 조합 글자의 앞 `n_units`(UTF-16 단위)가 격자에서 차지하는 칸 수. 후보창을 조합 중 글자 옆에 맞추는 데 쓴다.
///
/// Examples
/// --------
/// | 글자    | 앞 단위 수 | 칸 수 |
/// |---------|------------|-------|
/// | `한글`  | 1          | 2     |
/// | `한글`  | 2          | 4     |
/// | `a한`   | 1          | 1     |
pub fn cells_before(
    text: &str,
    n_units: usize,
) -> usize {
    let mut n_cells = 0;
    let mut n_seen = 0;
    for c in text.chars() {
        if n_seen < n_units {
            n_cells += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            n_seen += c.len_utf16();
        }
    }
    n_cells
}

/// 한 글자가 격자에서 차지하는 칸 수(1 또는 2).
pub fn cell_span(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(1).clamp(1, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composing(text: &str) -> ImeState {
        let mut state = ImeState::default();
        state.mark(text, None);
        state
    }

    #[test]
    fn test_a_name_ending_in_an_ascii_letter_does_not_swallow_the_first_typed_letter() {
        let sent = SentComposition::default();
        let now = Instant::now();
        sent.note_commit_elsewhere_at("build", now);
        assert!(!sent.is_duplicate_of_at("d", now), "a directly typed ASCII letter is never a late composition notice");
    }

    #[test]
    fn test_a_late_composition_notice_is_dropped_once_and_only_shortly_after_the_input_box_committed() {
        let sent = SentComposition::default();
        let now = Instant::now();
        sent.note_commit_elsewhere_at("이름 한", now);
        assert!(sent.is_duplicate_of_at("한", now + Duration::from_millis(50)));
        assert!(!sent.is_duplicate_of_at("한", now + Duration::from_millis(60)), "the memory is used up once");
        sent.note_commit_elsewhere_at("이름 한", now);
        assert!(
            !sent.is_duplicate_of_at("한", now + Duration::from_secs(3)),
            "a letter typed long afterwards is the user's own input"
        );
    }

    #[test]
    fn test_cells_before_counts_wide_chars_as_two() {
        assert_eq!(cells_before("한글", 0), 0);
        assert_eq!(cells_before("한글", 1), 2);
        assert_eq!(cells_before("한글", 2), 4);
        assert_eq!(cells_before("a한", 1), 1);
        assert_eq!(cells_before("한", 9), 2);
    }

    #[test]
    fn test_mark_replaces_text_and_empty_ends_composition() {
        let mut state = ImeState::default();
        assert_eq!(state.marked(), None);
        state.mark("ㅎ", None);
        assert_eq!(state.marked(), Some("ㅎ"));
        state.mark("하", None);
        assert_eq!(state.marked(), Some("하"));
        state.mark("", None);
        assert_eq!(state.marked(), None);
    }

    #[test]
    fn test_caret_defaults_to_end_in_utf16_units() {
        let mut state = ImeState::default();
        state.mark("한글", None);
        assert_eq!(state.caret_utf16(), 2);
        state.mark("한글", Some(1..1));
        assert_eq!(state.caret_utf16(), 1);
        // 글자 길이를 넘는 위치는 끝으로 맞춘다.
        state.mark("한", Some(5..5));
        assert_eq!(state.caret_utf16(), 1);
    }

    #[test]
    fn test_commit_sends_text_once_and_ends_composition() {
        let mut state = composing("한");
        assert_eq!(state.commit("한"), "한".as_bytes());
        assert_eq!(state.marked(), None);
        // 조합 중이 아닌 영문 확정도 그대로 나간다.
        assert_eq!(state.commit("a"), b"a");
    }

    #[test]
    fn test_commit_then_mark_keeps_next_composition_unsent() {
        // "한글"을 치다 ㄱ이 들어오면 입력 방식은 "한" 확정과 "ㄱ" 조합을 연달아 알린다.
        let mut state = composing("한");
        assert_eq!(state.commit("한"), "한".as_bytes());
        state.mark("ㄱ", None);
        assert_eq!(state.marked(), Some("ㄱ"));
    }

    #[test]
    fn test_empty_commit_cancels_without_bytes() {
        let mut state = composing("한");
        assert_eq!(state.commit(""), b"");
        assert_eq!(state.marked(), None);
    }

    #[test]
    fn test_enter_while_composing_flushes_text_before_enter() {
        let mut state = composing("글");
        let plan = state.plan_key("enter", true);
        assert_eq!(plan, KeyPlan { flushed: "글".as_bytes().to_vec(), send_key: true });
        assert_eq!(state.marked(), None);
        // 입력 방식이 같은 글자를 뒤늦게 확정으로 알려도 두 번 나가지 않는다.
        assert_eq!(state.commit("글"), b"");
        assert_eq!(state.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_enter_after_commit_sends_only_enter() {
        let mut state = composing("글");
        assert_eq!(state.commit("글"), "글".as_bytes());
        assert_eq!(state.plan_key("enter", true), KeyPlan { flushed: vec![], send_key: true });
    }

    #[test]
    fn test_backspace_is_left_to_input_method_while_composing() {
        let mut state = composing("한");
        assert_eq!(state.plan_key("backspace", true), KeyPlan { flushed: vec![], send_key: false });
        assert_eq!(state.marked(), Some("한"));
    }

    #[test]
    fn test_backspace_sends_when_not_composing() {
        let mut state = ImeState::default();
        assert_eq!(state.plan_key("backspace", true), KeyPlan { flushed: vec![], send_key: true });
    }

    #[test]
    fn test_other_key_while_composing_flushes_first() {
        let mut state = composing("한");
        assert_eq!(state.plan_key("left", true), KeyPlan { flushed: "한".as_bytes().to_vec(), send_key: true });
    }

    #[test]
    fn test_key_without_bytes_does_not_disturb_composition() {
        // 입력 방식이 처리 중인 키(조합 도중의 키 누름)는 보낼 바이트가 없어 조합을 건드리지 않는다.
        let mut state = composing("한");
        assert_eq!(state.plan_key("process", false), KeyPlan { flushed: vec![], send_key: false });
        assert_eq!(state.marked(), Some("한"));
    }

    #[test]
    fn test_flush_sends_composition() {
        let mut state = composing("한");
        assert_eq!(state.flush(), "한".as_bytes());
        assert_eq!(state.marked(), None);
        assert_eq!(state.commit("한"), b"");
        let mut idle = ImeState::default();
        assert_eq!(idle.flush(), b"");
    }

    #[test]
    fn test_new_composition_forgets_already_sent_text() {
        let mut state = composing("글");
        state.flush();
        state.mark("ㅎ", None);
        state.mark("", None);
        // 새 조합이 시작됐으니 같은 글자의 확정은 새 입력이다.
        assert_eq!(state.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_late_commit_after_a_tab_switch_is_dropped_by_the_tab_that_got_focus() {
        // 창 하나에 기억 하나: 탭 1이 포커스를 잃으며 조합 글자를 내보내고, 입력 방식이 같은 글자의 확정을 탭 2에 알려도 한 번만 나간다.
        let sent = SentComposition::default();
        let mut first = ImeState::new(sent.clone());
        let mut second = ImeState::new(sent);
        first.mark("글", None);
        assert_eq!(first.flush(), "글".as_bytes());
        assert_eq!(second.commit("글"), b"", "the late commit must not reach the second tab");
        // 한 번 버린 뒤의 같은 글자는 새 입력이다.
        assert_eq!(second.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_separate_windows_do_not_share_the_memory() {
        let mut first = ImeState::new(SentComposition::default());
        let mut other_window = ImeState::new(SentComposition::default());
        first.mark("글", None);
        first.flush();
        assert_eq!(other_window.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_c_d27_a_commit_made_in_another_input_is_dropped_once_when_it_reaches_the_terminal() {
        // 이름 바꾸기 입력칸이 Enter로 마지막 글자를 확정한 뒤 입력 방식이 같은 글자를 뒤늦게 터미널에 알려도 PTY로 가지 않는다.
        let sent = SentComposition::default();
        sent.note_commit_elsewhere("한글");
        let mut state = ImeState::new(sent.clone());
        assert_eq!(state.commit("글"), Vec::<u8>::new());
        // 한 번만 버린다: 이어지는 같은 글자는 사용자가 친 것이다.
        assert_eq!(state.commit("글"), "글".as_bytes());
        // 빈 입력칸은 기억할 글자가 없다.
        sent.note_commit_elsewhere("");
        assert_eq!(state.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_c_d27_a_key_or_new_composition_cancels_the_wait_for_a_commit_made_elsewhere() {
        let sent = SentComposition::default();
        let mut state = ImeState::new(sent.clone());
        sent.note_commit_elsewhere("글");
        state.plan_key("enter", true);
        assert_eq!(state.commit("글"), "글".as_bytes());
        sent.note_commit_elsewhere("글");
        state.mark("ㄱ", None);
        assert_eq!(state.commit("글"), "글".as_bytes());
    }

    #[test]
    fn test_c_d27_flushing_a_composition_before_a_dialog_opens_sends_it_and_drops_its_late_commit() {
        // 조합 중 우클릭 → Rename: 열기 전에 조합 글자를 터미널로 확정해 두면, 입력 방식이 뒤늦게 같은 글자를 알려도 한 번만 나간다.
        let sent = SentComposition::default();
        let mut state = ImeState::new(sent);
        state.mark("한", None);
        assert_eq!(state.flush(), "한".as_bytes());
        assert_eq!(state.marked(), None);
        assert_eq!(state.commit("한"), Vec::<u8>::new());
    }
}
