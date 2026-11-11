//! 자동 점검용: 폴더 검색의 바꾸기(Replace in Files) 대본 명령(`act`)과 창 상태 글 줄. `automation` 기능으로 빌드한 개발용 실행 파일에만 들어간다.
use gpui_kit::{Context, Window};

use crate::window_view::WorkWindow;

impl WorkWindow {
    /// 창 상태 글에 붙는 바꾸기 줄: `search-replace open=y|n text=<글>`.
    pub(crate) fn replace_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> String {
        format!(
            "search-replace open={} text={}",
            if self.search.replace_open { "y" } else { "n" },
            self.replacement_text(cx).replace(' ', "_")
        )
    }

    /// 바꾸기의 `act` 한 줄을 실행한다. 이 파일이 아는 명령이면 true다.
    ///
    /// | 명령               | 하는 일                                                              |
    /// |--------------------|----------------------------------------------------------------------|
    /// | `replaceopen`      | `⇄` 단추를 누른 것과 같다(바꾸기 줄 열기·닫기)                       |
    /// | `replacetext 글`   | 바꿀 글 입력칸에 글을 넣는다(바꾸기 줄이 닫혀 있으면 연다)           |
    /// | `replacefile N`    | 결과의 N번째 파일 행의 `Replace`를 누른 것과 같다                     |
    /// | `replaceall`       | `Replace All`을 누른 것과 같다(확인 창이 뜨고 `act confirm`이 승인)   |
    pub(crate) fn replace_action(
        &mut self,
        parts: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut handled = true;
        match parts {
            ["replaceopen"] => self.toggle_replace(window, cx),
            ["replacetext", text @ ..] => {
                if !self.search.replace_open {
                    self.toggle_replace(window, cx);
                }
                if let Some(input) = self.search.replace_input.clone() {
                    input.update(cx, |state, cx| state.set_value(text.join(" "), window, cx));
                }
            }
            ["replacefile", n] => {
                if let Some(file) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
                    self.replace_file(file, window, cx);
                }
            }
            ["replaceall"] => self.request_replace_all(window, cx),
            _ => handled = false,
        }
        handled
    }
}
