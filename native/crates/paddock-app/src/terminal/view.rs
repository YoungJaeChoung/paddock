//! 터미널 보기: 격자를 그리는 canvas와 키·마우스·스크롤 이벤트를 격자와 데몬 입력으로 잇는다.
//! 글자는 키 이벤트가 아니라 입력 방식(IME) 경로로 받아 한글 조합 중 글자를 PTY에 보내지 않고 커서 자리에 미리 보여 준다.
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Render, ScrollWheelEvent, Styled, Subscription, UTF16Selection, Window, canvas, div, point,
    px, size,
};
use paddock_core::app_shortcut::terminal_key_bytes;
use paddock_core::key_encoding::{ClipboardKey, KeyInput, clipboard_key, paste_bytes};
use paddock_core::theme::terminal as terminal_size;

use crate::chrome::zpx;
use crate::terminal::grid::Grid;
use crate::terminal::ime::{ImeState, SentComposition, cells_before};
use crate::terminal::paint::{GlyphCache, PaintInfo, PaintInputs, hsla, paint_grid};
use crate::terminal::palette::default_bg;

/// 출력이 들어왔는지 확인해 다시 그리는 간격(약 60Hz).
const REFRESH_INTERVAL: Duration = Duration::from_millis(16);

pub struct TerminalView {
    pub grid: Arc<Grid>,
    focus: FocusHandle,
    cache: Rc<RefCell<GlyphCache>>,
    info: Rc<RefCell<PaintInfo>>,
    selecting: bool,
    /// 한글 조합 상태. 조합 중 글자는 확정될 때까지 PTY에 보내지 않는다.
    ime: ImeState,
    /// 포커스·창 활성 변화를 듣는 구독. 보기가 살아 있는 동안 유지한다.
    _subscriptions: Vec<Subscription>,
}

impl TerminalView {
    /// `cache`는 글리프 모양과 그리기 버퍼로, 같은 창의 터미널 보기들이 함께 쓴다(글꼴이 같으므로).
    /// `already_sent`는 키·포커스 이동으로 내보낸 조합 글자의 기억으로, 같은 창의 보기들이 함께 쥔다.
    pub fn new(
        grid: Arc<Grid>,
        cache: Rc<RefCell<GlyphCache>>,
        already_sent: SentComposition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            grid: grid.clone(),
            focus,
            cache,
            info: Rc::new(RefCell::new(PaintInfo::default())),
            selecting: false,
            ime: ImeState::new(already_sent),
            _subscriptions: Vec::new(),
        };
        // 포커스가 다른 곳으로 가거나 창이 비활성이 되면 조합 글자를 확정한다.
        this._subscriptions = vec![
            cx.on_blur(&this.focus, window, |view, _window, cx| view.flush_composition(cx)),
            cx.observe_window_activation(window, |view, window, cx| {
                if !window.is_window_active() {
                    view.flush_composition(cx);
                }
            }),
        ];
        // 출력이 들어오면(dirty) 다시 그린다. 보기가 사라지면(탭을 닫으면) 끝난다.
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                let dirty = grid.shared.dirty.swap(false, Ordering::Relaxed);
                if view
                    .update(cx, |_, cx| {
                        if dirty {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }

    /// 창 좌표를 (격자 칸, 칸의 왼쪽/오른쪽 절반)으로 바꾼다. 스크롤한 만큼 보정한다.
    fn cell_at(
        &self,
        x: Pixels,
        y: Pixels,
    ) -> (GridPoint, Side) {
        let info = *self.info.borrow();
        let dx = f32::from(x) - info.origin_x;
        let dy = f32::from(y) - info.origin_y;
        let col = ((dx / info.cell_w).floor().max(0.0) as usize).min(info.cols.saturating_sub(1) as usize);
        let row = ((dy / info.cell_h).floor().max(0.0) as i32).min(info.rows as i32 - 1).max(0);
        let side = if (dx - col as f32 * info.cell_w) < info.cell_w / 2.0 { Side::Left } else { Side::Right };
        let display_offset = self.grid.term.lock().grid().display_offset() as i32;
        (GridPoint::new(Line(row - display_offset), Column(col)), side)
    }

    fn on_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        // 비라틴 배열의 글자 키는 같은 자리의 영문 이름으로 바꿔 단축키·클립보드 키를 맞춘다.
        let key_name = crate::platform_keys::shortcut_key(&keystroke.key);
        let input = KeyInput {
            key: key_name.as_str(),
            key_char: keystroke.key_char.as_deref(),
            control: keystroke.modifiers.control,
            shift: keystroke.modifiers.shift,
            alt: keystroke.modifiers.alt,
        };
        // Ctrl+C는 선택한 글자가 있을 때만 복사이고 없으면 셸의 ^C다. Ctrl+V는 붙여넣기다. Ctrl+Shift+C는 분할 키라 복사가 아니다.
        let selected = self.grid.term.lock().selection_to_string();
        let clipboard = clipboard_key(&input, selected.is_some());
        if clipboard == Some(ClipboardKey::Copy) {
            if let Some(text) = selected {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        } else if clipboard == Some(ClipboardKey::Paste) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let bracketed = self.grid.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
                self.grid.input(paste_bytes(&text, bracketed));
            }
        } else {
            let app_cursor = self.grid.term.lock().mode().contains(TermMode::APP_CURSOR);
            // 글자를 만드는 키(Ctrl·Alt 없이 누른 글자, AltGr 조합)는 여기서 보내지 않는다.
            // 같은 글자가 입력 방식(IME) 경로로 한 번 더 오므로, 그쪽이 보내야 두 번 나가지 않는다.
            let makes_text = keystroke.key_char.as_deref().is_some_and(|text| !text.chars().any(char::is_control))
                && ((!input.control && !input.alt) || event.prefer_character_input);
            #[cfg(feature = "automation")]
            crate::automation::event(&format!(
                "key {:?} char={:?} text_path={makes_text} marked={:?}",
                keystroke.key,
                keystroke.key_char,
                self.ime.marked()
            ));
            if !makes_text {
                let key_bytes = terminal_key_bytes(&input, app_cursor);
                let plan = self.ime.plan_key(keystroke.key.as_str(), key_bytes.is_some());
                let mut bytes = plan.flushed;
                if plan.send_key {
                    bytes.extend(key_bytes.unwrap_or_default());
                }
                if !bytes.is_empty() {
                    self.send(bytes, cx);
                    // 글자 없는 키는 입력 방식 경로로 한 번 더 오지 않게 여기서 끝낸다.
                    cx.stop_propagation();
                }
            }
        }
    }

    /// PTY로 바이트를 보낸다. 입력이 있으면 스크롤백을 보던 화면이 맨 아래로 돌아온다.
    fn send(
        &mut self,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        self.grid.term.lock().scroll_display(Scroll::Bottom);
        self.grid.input(bytes);
        cx.notify();
    }

    /// 조합 중인 글자를 확정으로 보낸다. 포커스가 떠나거나 입력 방식이 조합을 그대로 두라고 할 때 쓴다.
    pub(crate) fn flush_composition(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let bytes = self.ime.flush();
        #[cfg(feature = "automation")]
        crate::automation::event(&format!("ime flush -> {} bytes", bytes.len()));
        if !bytes.is_empty() {
            self.send(bytes, cx);
        }
        cx.notify();
    }
}

/// UTF-16 단위 범위를 글자 범위로 바꿔 `text`에서 잘라 낸다. 범위가 글자 길이를 넘으면 잘라 맞춘다.
fn slice_utf16(
    text: &str,
    range: &Range<usize>,
) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = range.end.min(units.len());
    let start = range.start.min(end);
    String::from_utf16_lossy(&units[start..end])
}

/// 터미널 보기가 입력 방식(IME)에서 받는 글자 경로. 조합 중 글자는 `ime`에 두고, 확정 때만 PTY로 보낸다.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.ime.marked()?;
        let n_units = marked.encode_utf16().count();
        *adjusted_range = Some(range.start.min(n_units)..range.end.min(n_units));
        Some(slice_utf16(marked, &range))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let caret = self.ime.caret_utf16();
        Some(UTF16Selection { range: caret..caret, reversed: false })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.ime.marked().map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_composition(cx);
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bytes = self.ime.commit(text);
        #[cfg(feature = "automation")]
        crate::automation::event(&format!("ime commit {text:?} -> {} bytes", bytes.len()));
        if !bytes.is_empty() {
            self.send(bytes, cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(feature = "automation")]
        crate::automation::event(&format!("ime mark {new_text:?}"));
        self.ime.mark(new_text, new_selected_range);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // 커서 칸에서 조합 글자의 앞부분이 차지한 칸만큼 오른쪽 칸을 돌려준다. 후보창이 그 옆에 뜬다.
        let info = *self.info.borrow();
        let (row, col) = {
            let term = self.grid.term.lock();
            let cursor = term.grid().cursor.point;
            (cursor.line.0 + term.grid().display_offset() as i32, cursor.column.0)
        };
        let n_before = self.ime.marked().map_or(0, |text| cells_before(text, range_utf16.start));
        let x = info.origin_x + (col + n_before) as f32 * info.cell_w;
        let y = info.origin_y + row as f32 * info.cell_h;
        Some(Bounds::new(point(px(x), px(y)), size(px(info.cell_w), px(info.cell_h))))
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui_kit::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Focusable for TerminalView {
    fn focus_handle(
        &self,
        _cx: &App,
    ) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let inputs = PaintInputs {
            grid: self.grid.clone(),
            cache: self.cache.clone(),
            info: self.info.clone(),
            focused: self.focus.is_focused(window),
            marked: self.ime.marked().map(str::to_string),
        };
        let view = cx.entity();
        let focus = self.focus.clone();
        div()
            .size_full()
            .px(zpx(terminal_size::PADDING_X))
            .py(zpx(terminal_size::PADDING_Y))
            .bg(hsla(default_bg()))
            .track_focus(&self.focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    let (point, side) = this.cell_at(event.position.x, event.position.y);
                    this.grid.term.lock().selection = Some(Selection::new(SelectionType::Simple, point, side));
                    this.selecting = true;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.selecting {
                    let (point, side) = this.cell_at(event.position.x, event.position.y);
                    if let Some(selection) = this.grid.term.lock().selection.as_mut() {
                        selection.update(point, side);
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, _cx| {
                    this.selecting = false;
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                let cell_h = this.info.borrow().cell_h.max(1.0);
                let dy = f32::from(event.delta.pixel_delta(px(cell_h)).y);
                let n_lines = (dy / cell_h).round() as i32;
                if n_lines != 0 {
                    this.grid.term.lock().scroll_display(Scroll::Delta(n_lines));
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    |bounds, _window, _cx| bounds,
                    move |_bounds, bounds: Bounds<Pixels>, window, cx| {
                        // 그리는 사이에 입력 방식 경로를 이 보기에 연결한다(포커스가 있을 때만 글자를 받는다).
                        window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
                        paint_grid(&inputs, bounds, window, cx);
                    },
                )
                .size_full(),
            )
    }
}
