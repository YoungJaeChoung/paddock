//! 터미널 안 찾기 띠: `Ctrl+Shift+H`로 선택한 터미널 칸의 오른쪽 위에 여는 상자(입력칸 · `N of M` · ‹ › · ×)와 그 동작.
//!
//! 입력칸이 키 입력을 받는 동안 키는 터미널(PTY)로 가지 않는다. Enter는 다음 일치, Shift+Enter는 이전 일치, Esc는 띠를 닫고 터미널로 돌아간다.
//! 일치는 터미널 격자의 선택으로 강조하고 그 줄로 스크롤한다. 검색 자체는 `find`가 한다.
use alacritty_terminal::term::search::Match;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AppContext, Context, Div, Entity, Focusable, InteractiveElement, MouseButton, MouseDownEvent, ParentElement,
    Stateful, Styled, Window, div,
};
use paddock_core::terminal_find::FindState;
use paddock_core::theme::terminal_find as size;

use super::find::{find_matches, show_match};
use crate::chrome::zpx;
use crate::look::colors;
use crate::terminal::paint::hsla;
use crate::window_view::WorkWindow;

/// 열려 있는 찾기 띠. 띠는 하나의 터미널에 붙어 있고, 그 터미널이 칸에 보이는 동안만 그려진다.
pub struct FindBar {
    /// 찾는 터미널.
    pub terminal: u32,
    pub state: FindState,
    pub input: Entity<InputState>,
    /// 질의와 일치하는 구간들(화면 순서).
    pub matches: Vec<Match>,
}

/// ‹ › × 단추 하나.
fn bar_button(
    id: &'static str,
    glyph: &'static str,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(zpx(size::BUTTON_SIZE))
        .h(zpx(size::BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded(zpx(size::BUTTON_RADIUS))
        .text_color(hsla(colors().fg_muted))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(colors().hover_bg)).text_color(hsla(colors().fg_strong)))
        .child(glyph)
}

/// 터미널 `terminal`에 붙은 찾기 띠가 있으면 그린 것. 띠 안의 누름은 터미널 칸 선택으로 번지지 않는다(번지면 포커스가 터미널로 돌아간다).
pub fn find_overlay(
    window_view: &WorkWindow,
    terminal: u32,
    cx: &mut Context<WorkWindow>,
) -> Option<Div> {
    window_view.find.as_ref().filter(|bar| bar.terminal == terminal).map(|bar| {
        find_bar_view(
            &bar.input,
            &bar.state.count_text(),
            cx,
            |this: &mut WorkWindow, forward, _window, cx| this.find_step(forward, cx),
            |this: &mut WorkWindow, window, cx| this.close_terminal_find(window, cx),
        )
    })
}

/// 찾기 띠 틀: 입력칸 · `N of M` · ‹ › · ×. 단추가 부르는 동작(`on_step`은 다음이면 true, `on_close`)은 이 띠를 그리는 보기(`T`)가 정한다 —
/// 본 창의 띠는 본 창이, 분리 창의 띠는 분리 창이 본 창에 시킨다.
pub fn find_bar_view<T: 'static>(
    input: &Entity<InputState>,
    count: &str,
    cx: &mut Context<T>,
    on_step: impl Fn(&mut T, bool, &mut Window, &mut Context<T>) + Clone + 'static,
    on_close: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> Div {
    let palette = colors();
    let stop = || {
        cx.listener(|_this: &mut T, _event: &MouseDownEvent, _window, cx| {
            cx.stop_propagation();
        })
    };
    let (step_back, step_forward) = (on_step.clone(), on_step);
    div()
        .absolute()
        .top(zpx(size::MARGIN_TOP))
        .right(zpx(size::MARGIN_RIGHT))
        .w(zpx(size::WIDTH))
        .h(zpx(size::HEIGHT))
        .px(zpx(size::PADDING_X))
        .flex()
        .flex_row()
        .items_center()
        .gap(zpx(size::GAP))
        .rounded(zpx(size::RADIUS))
        .border_1()
        .border_color(hsla(palette.popup_border))
        .bg(hsla(palette.popup_bg))
        .text_color(hsla(palette.fg))
        .shadow_lg()
        .occlude()
        .on_mouse_down(MouseButton::Left, stop())
        .child(div().flex_1().min_w_0().child(Input::new(input).appearance(false).bordered(false)))
        .child(
            div()
                .flex_none()
                .min_w(zpx(size::COUNT_MIN_WIDTH))
                .text_size(zpx(size::COUNT_SIZE))
                .text_color(hsla(palette.fg_muted))
                .child(count.to_string()),
        )
        .child(bar_button("find-previous", "\u{2039}").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this: &mut T, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                step_back(this, false, window, cx);
            }),
        ))
        .child(bar_button("find-next", "\u{203a}").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this: &mut T, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                step_forward(this, true, window, cx);
            }),
        ))
        .child(bar_button("find-close", "\u{2715}").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this: &mut T, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                on_close(this, window, cx);
            }),
        ))
}

impl WorkWindow {
    /// `Ctrl+Shift+H`: 선택한 터미널에 찾기 띠를 열고 입력칸에 키 입력을 준다. 이미 열려 있으면 입력칸 글자를 모두 선택한다.
    /// 파일 탭이 활성이거나 터미널이 없으면 아무것도 하지 않는다.
    pub(crate) fn open_terminal_find(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.workspace.selected().filter(|_| !self.file_active());
        if let Some(terminal) = terminal {
            self.open_terminal_find_for(terminal, window, cx);
        }
    }

    /// 터미널 `terminal`에 찾기 띠를 열고 입력칸에 키 입력을 준다. `window`는 띠가 그려질 창이다 — 본 창이거나 그 터미널이 있는 분리 창이다.
    /// 찾기 상태는 본 창이 하나만 쥐므로 다른 터미널의 띠가 열려 있었으면 닫힌다.
    pub(crate) fn open_terminal_find_for(
        &mut self,
        terminal: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.tab(terminal).is_some() {
            if self.find.as_ref().is_none_or(|bar| bar.terminal != terminal) {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find"));
                cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| {
                    if this.find.as_ref().is_some_and(|bar| &bar.input == input) {
                        match event {
                            InputEvent::Change => this.run_find(window, cx),
                            InputEvent::PressEnter { shift, .. } => this.find_step(!*shift, cx),
                            InputEvent::Focus | InputEvent::Blur => {}
                        }
                    }
                })
                .detach();
                self.find = Some(FindBar { terminal, state: FindState::default(), input, matches: Vec::new() });
            }
            if let Some(bar) = self.find.as_ref() {
                bar.input.update(cx, |state, cx| {
                    state.focus(window, cx);
                    state.select_all(window, cx);
                });
            }
            cx.notify();
        }
    }

    /// 입력칸의 질의로 스크롤백까지 다시 찾고 첫 일치를 강조한다. 질의가 비면 일치도 강조도 없다.
    pub(crate) fn run_find(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = self.find.as_ref().map(|bar| bar.input.read(cx).value().to_string()).unwrap_or_default();
        let grid = self
            .find
            .as_ref()
            .and_then(|bar| self.workspace.tab(bar.terminal))
            .map(|tab| tab.content.session.grid.clone());
        if let (Some(grid), Some(bar)) = (grid, self.find.as_mut()) {
            bar.matches = find_matches(&grid, &query);
            bar.state.set_results(&query, bar.matches.len());
            match bar.state.current.and_then(|at| bar.matches.get(at)) {
                Some(found) => show_match(&grid, found),
                None => grid.term.lock().selection = None,
            }
            cx.notify();
        }
    }

    /// 다음(`forward`) 또는 이전 일치로 가서 강조한다. 일치가 없으면 아무것도 하지 않는다.
    pub(crate) fn find_step(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) {
        let grid = self
            .find
            .as_ref()
            .and_then(|bar| self.workspace.tab(bar.terminal))
            .map(|tab| tab.content.session.grid.clone());
        if let (Some(grid), Some(bar)) = (grid, self.find.as_mut()) {
            if forward {
                bar.state.next();
            } else {
                bar.state.previous();
            }
            if let Some(found) = bar.state.current.and_then(|at| bar.matches.get(at)) {
                show_match(&grid, found);
            }
            cx.notify();
        }
    }

    /// 찾기 띠를 닫고 강조를 지우고 키 입력을 선택한 탭으로 돌린다.
    pub(crate) fn close_terminal_find(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bar) = self.find.take() {
            if let Some(tab) = self.workspace.tab(bar.terminal) {
                tab.content.session.grid.term.lock().selection = None;
            }
            // 다른 창에 있는 터미널의 띠였으면 키 입력은 그 터미널(이 `window`)로 돌아간다.
            match self.workspace.tab(bar.terminal).filter(|_| self.workspace.is_detached(bar.terminal)) {
                Some(tab) => window.focus(&tab.content.view.focus_handle(cx), cx),
                None => self.focus_selected(window, cx),
            }
            cx.notify();
        }
    }

    /// 찾기 띠의 입력칸이 지금 키 입력을 받고 있는지.
    pub(crate) fn find_input_focused(
        &self,
        window: &Window,
        cx: &gpui_kit::App,
    ) -> bool {
        self.find.as_ref().is_some_and(|bar| bar.input.focus_handle(cx).is_focused(window))
    }
}
