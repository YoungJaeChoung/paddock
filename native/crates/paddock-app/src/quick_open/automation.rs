//! 자동 점검용: 빠른 열기의 대본 명령(`act`)과 창 상태 글 줄. `automation` 기능으로 빌드한 개발용 실행 파일에만 들어간다.
use gpui_kit::component::input::InputState;
use gpui_kit::{Context, Window};

use crate::window_view::{Overlay, WorkWindow};

impl WorkWindow {
    /// 창 상태 글에 붙는 빠른 열기 줄: `quick-open open=y|n …`과 보이는 줄마다 `quick-row 번호 경로`(앞의 10개), 강조한 줄 `quick-highlight=번호|-`.
    pub(crate) fn quick_open_summary(
        &self,
        cx: &gpui_kit::App,
    ) -> Vec<String> {
        match self.quick_open.as_ref() {
            None => vec!["quick-open open=n".to_string()],
            Some(quick) => {
                let mut lines = vec![
                    format!(
                        "quick-open open=y indexing={} capped={} files={} shown={} query={}",
                        if quick.indexing { "y" } else { "n" },
                        if quick.capped { "y" } else { "n" },
                        quick.files.len(),
                        quick.n_shown(),
                        quick.input.read(cx).value().replace(' ', "_")
                    ),
                    format!(
                        "quick-highlight={}",
                        quick.nav.highlight.map_or("-".to_string(), |at| (at + 1).to_string())
                    ),
                ];
                lines.extend(
                    quick.shown().take(10).enumerate().map(|(index, path)| format!("quick-row {} {path}", index + 1)),
                );
                lines
            }
        }
    }

    /// 빠른 열기의 `act` 한 줄을 실행한다. 이 파일이 아는 명령이면 true다.
    ///
    /// | 명령              | 하는 일                                                                  |
    /// |-------------------|--------------------------------------------------------------------------|
    /// | `quickopen 질의`  | Ctrl+Shift+P로 열고(열려 있으면 그대로) 입력칸에 질의를 넣는다           |
    /// | `quickpick N`     | 보이는 N번째(1부터) 줄을 눌러 연다                                       |
    /// | `quickclose`      | Esc로 닫는다                                                             |
    pub(crate) fn quick_open_action(
        &mut self,
        parts: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut handled = true;
        match parts {
            ["quickopen", query @ ..] => {
                if self.overlay != Overlay::QuickOpen {
                    self.toggle_quick_open(window, cx);
                }
                let input = self.quick_open.as_ref().map(|quick| quick.input.clone());
                if let Some(input) = input {
                    input.update(cx, |state: &mut InputState, cx| state.set_value(query.join(" "), window, cx));
                    self.quick_open_refresh(cx);
                }
            }
            ["quickpick", n] => {
                if let Some(index) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
                    self.quick_open_pick(Some(index), window, cx);
                }
            }
            ["quickclose"] => {
                if self.overlay == Overlay::QuickOpen {
                    self.cancel_overlay(window, cx);
                }
            }
            _ => handled = false,
        }
        handled
    }
}
