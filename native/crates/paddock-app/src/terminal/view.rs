//! 터미널 보기: 격자를 그리는 canvas와 키·마우스·스크롤 이벤트를 격자와 데몬 입력으로 잇는다.
//! 글자는 키 이벤트가 아니라 입력 방식(IME) 경로로 받아 한글 조합 중 글자를 PTY에 보내지 않고 커서 자리에 미리 보여 준다.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, Subscription, UTF16Selection, Window, canvas, div, point, px, size,
};
use paddock_core::app_shortcut::terminal_key_bytes;
use paddock_core::incoming::{Incoming, incoming_of};
use paddock_core::key_encoding::{ClipboardKey, KeyInput, clipboard_key, copy_on_selection_text, paste_bytes};
use paddock_core::paste_paths::{ImageKind, PastePlace, paste_path_text};
use paddock_core::theme::terminal as terminal_size;

use crate::chrome::zpx;
use crate::files::clipboard::{parts_of, text_of};
use crate::terminal::grid::Grid;
use crate::terminal::ime::{ImeState, SentComposition, cells_before};
use crate::terminal::paint::{GlyphCache, PaintInfo, PaintInputs, hsla, paint_grid};
use crate::terminal::palette::default_bg;
use crate::terminal_link::{Target, home_folder, is_file, link_at, links_in_row, target_of, terminal_cwd};

/// 출력이 들어왔는지 확인해 다시 그리는 간격(약 60Hz).
const REFRESH_INTERVAL: Duration = Duration::from_millis(16);

/// 경로가 파일인지 확인한 결과(`file_checks`)의 기억 규칙.
mod link_check {
    use std::time::Duration;

    /// 확인한 결과를 믿는 시간. 지나면 다시 확인한다(그 사이 파일이 생기거나 지워질 수 있다).
    pub const TTL: Duration = Duration::from_secs(3);

    /// 확인해 둔 경로 수가 이 개수를 넘으면 비운다.
    pub const LIMIT: usize = 500;
}

/// 사용자가 파일 경로 링크를 Ctrl+클릭했다. 창이 그 파일을 파일 탭으로 연다.
pub struct OpenLink {
    pub target: Target,
}

/// Ctrl을 누른 채 올린 글자가 가리키는 실제 파일.
struct HoverLink {
    /// 화면 줄(위에서 0부터).
    row: i32,
    columns: Range<usize>,
    target: Target,
}

pub struct TerminalView {
    pub grid: Arc<Grid>,
    focus: FocusHandle,
    cache: Rc<RefCell<GlyphCache>>,
    info: Rc<RefCell<PaintInfo>>,
    selecting: bool,
    /// 한글 조합 상태. 조합 중 글자는 확정될 때까지 PTY에 보내지 않는다.
    ime: ImeState,
    /// Ctrl을 누른 채 올린 글자가 실제 파일을 가리킬 때의 링크. 밑줄이 그어지고 Ctrl+클릭이 이 파일을 연다.
    link: Option<HoverLink>,
    /// 마우스의 마지막 위치와 Ctrl이 눌려 있는지. Ctrl만 누르거나 떼도 링크를 다시 정하는 데 쓴다.
    pointer: Option<(Pixels, Pixels)>,
    ctrl: bool,
    /// 경로가 파일인지 확인한 결과(확인한 시각, 파일 여부). 확인은 작업 스레드에서 하고 창은 결과만 쓴다.
    file_checks: HashMap<String, (Instant, bool)>,
    /// 지금 확인 중인 경로.
    checks_in_flight: HashSet<String>,
    /// 파일 경로를 붙여 넣을 때 이 터미널의 셸이 읽는 모양(WSL이면 `/mnt/…`, Windows 셸이면 큰따옴표, 그 밖은 `\` 이스케이프).
    paste_place: PastePlace,
    /// 포커스·창 활성 변화를 듣는 구독. 보기가 살아 있는 동안 유지한다.
    _subscriptions: Vec<Subscription>,
}

/// 이 터미널로 글자가 나갔다(키·입력 방식·붙여넣기 어느 경로든). 창이 탭 줄 초점을 거두는 데 쓴다.
pub struct Typed;

/// 경로 없는 클립보드 그림을 이 터미널에 붙여 넣으려 한다. 창이 쓰기 길로 저장한 뒤 경로를 붙여 넣는다.
pub struct PasteImage {
    pub kind: ImageKind,
    pub bytes: Vec<u8>,
}

/// 제어 문자가 든 경로라 붙이지 않았다. 창이 상태 줄에 알린다.
pub struct PasteRefused;

impl EventEmitter<OpenLink> for TerminalView {}
impl EventEmitter<Typed> for TerminalView {}
impl EventEmitter<PasteImage> for TerminalView {}
impl EventEmitter<PasteRefused> for TerminalView {}

impl TerminalView {
    /// `cache`는 글리프 모양과 그리기 버퍼로, 같은 창의 터미널 보기들이 함께 쓴다(글꼴이 같으므로).
    /// `already_sent`는 키·포커스 이동으로 내보낸 조합 글자의 기억으로, 같은 창의 보기들이 함께 쥔다.
    pub fn new(
        grid: Arc<Grid>,
        cache: Rc<RefCell<GlyphCache>>,
        already_sent: SentComposition,
        paste_place: PastePlace,
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
            link: None,
            pointer: None,
            ctrl: false,
            file_checks: HashMap::new(),
            checks_in_flight: HashSet::new(),
            paste_place,
            _subscriptions: Vec::new(),
        };
        this._subscriptions = this.window_subscriptions(window, cx);
        // 출력이 들어오면(dirty) 다시 그린다. 보기가 사라지면(탭을 닫으면) 끝난다.
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                let dirty = grid.shared.dirty.swap(false, Ordering::Relaxed);
                if view
                    .update(cx, |view, cx| {
                        if dirty {
                            // 출력이 글자를 바꿨으면 밑줄 자리도 다시 정한다.
                            view.refresh_link(cx);
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

    /// 포커스가 다른 곳으로 가거나 창이 비활성이 되면 조합 글자를 확정하는 구독. 둘 다 `window`에 묶이므로 보기가 다른 창으로 옮겨 가면 다시 건다.
    fn window_subscriptions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        vec![
            cx.on_blur(&self.focus, window, |view, _window, cx| view.flush_composition(cx)),
            cx.observe_window_activation(window, |view, window, cx| {
                if !window.is_window_active() {
                    view.flush_composition(cx);
                }
            }),
        ]
    }

    /// 이 보기를 다른 창으로 옮긴다: 새 창이 쥔 글리프 캐시와 조합 기억으로 바꾸고, 창에 묶인 구독을 `window`에 다시 건다.
    ///
    /// 터미널 세션과 격자는 창과 무관하므로 그대로다. 글리프 캐시는 창의 배율에 맞춰 잡히고 조합 기억은 창 하나에 하나여야 해서
    /// (한 창에서 내보낸 조합 글자의 뒤늦은 확정이 다른 창의 보기에 한 번 더 나가면 안 된다) 창마다 따로 쥔다. 조합 중이던 글자는
    /// 옮기기 전에 확정해 PTY로 보낸다.
    pub fn rehome(
        &mut self,
        cache: Rc<RefCell<GlyphCache>>,
        already_sent: SentComposition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_composition(cx);
        self.cache = cache;
        self.ime = ImeState::new(already_sent);
        self.selecting = false;
        self._subscriptions = self.window_subscriptions(window, cx);
        cx.notify();
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

    /// 마우스 아래 글자가 실제 파일을 가리키는 경로이고 Ctrl이 눌려 있으면 링크로 정하고, 아니면 링크를 지운다.
    /// 파일 여부는 작업 스레드에서 확인한다 — 확인 결과가 오기 전에는 링크가 없고, 오면 다시 이 함수가 불려 밑줄이 생긴다.
    fn refresh_link(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let before = self.link.as_ref().map(|link| (link.row, link.columns.clone()));
        let mut found: Option<HoverLink> = None;
        if let (true, Some((x, y))) = (self.ctrl, self.pointer) {
            let (point, _) = self.cell_at(x, y);
            let row = point.line.0 + self.grid.term.lock().grid().display_offset() as i32;
            let (text, columns) = self.grid.row_cells(row);
            let links = links_in_row(&text, &columns);
            let target = link_at(&links, point.column.0).and_then(|link| {
                target_of(link, terminal_cwd(&self.grid).as_deref(), home_folder().as_deref())
                    .map(|target| (link.columns.clone(), target))
            });
            if let Some((columns, target)) = target {
                match self.file_checks.get(&target.path) {
                    Some((at, is_file)) if at.elapsed() < link_check::TTL => {
                        if *is_file {
                            found = Some(HoverLink { row, columns, target });
                        }
                    }
                    _ => self.check_file(target.path, cx),
                }
            }
        }
        self.link = found;
        if before != self.link.as_ref().map(|link| (link.row, link.columns.clone())) {
            cx.notify();
        }
    }

    /// 지금 화면에서 실제 파일을 가리키는 경로 링크를 화면 순서로 돌려준다(자동 점검용). 파일인지는 올려 보기와 같은 확인 기억(`file_checks`)만 읽고,
    /// 아직 확인하지 않았거나 기억이 낡은 경로는 작업 스레드에서 확인을 시작한다 — 창 스레드가 파일 시스템을 읽지 않는다.
    #[cfg(feature = "automation")]
    pub fn checked_targets(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Vec<Target> {
        let mut found = Vec::new();
        for target in crate::terminal_link::screen_targets(&self.grid) {
            match self.file_checks.get(&target.path).copied() {
                Some((at, is_file)) => {
                    if at.elapsed() >= link_check::TTL {
                        self.check_file(target.path.clone(), cx);
                    }
                    if is_file {
                        found.push(target);
                    }
                }
                None => self.check_file(target.path.clone(), cx),
            }
        }
        found
    }

    /// 경로가 파일인지 작업 스레드에서 확인하고 결과를 기억한 뒤 링크를 다시 정한다. 같은 경로를 이미 확인 중이면 새로 시작하지 않는다.
    fn check_file(
        &mut self,
        path: String,
        cx: &mut Context<Self>,
    ) {
        if self.checks_in_flight.insert(path.clone()) {
            cx.spawn(async move |view, cx| {
                let asked = path.clone();
                let exists = cx.background_executor().spawn(async move { is_file(&asked) }).await;
                let _ = view.update(cx, |view, cx| {
                    view.checks_in_flight.remove(&path);
                    if view.file_checks.len() > link_check::LIMIT {
                        view.file_checks.clear();
                    }
                    view.file_checks.insert(path, (Instant::now(), exists));
                    view.refresh_link(cx);
                });
            })
            .detach();
        }
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
            self.paste_clipboard(cx);
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

    /// 글자를 터미널에 붙여 넣는다(프로그램이 붙여넣기 감싸기를 켰으면 감싼다). 입력이 있었으니 선택은 풀리고 화면은 맨 아래로 돌아온다.
    pub fn paste_text(
        &mut self,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let bracketed = self.grid.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        self.send(paste_bytes(text, bracketed), cx);
    }

    /// Ctrl+V: 클립보드에 글자가 있으면 글자를, 없고 파일이 있으면 그 경로를, 그림만 있으면 저장한 그림의 경로를 붙여 넣는다.
    pub fn paste_clipboard(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let item = cx.read_from_clipboard();
        match item.as_ref().and_then(text_of) {
            Some(text) => self.paste_text(&text, cx),
            None => match incoming_of(&item.as_ref().map(parts_of).unwrap_or_default()) {
                Some(Incoming::Paths(paths)) => self.paste_paths(&paths, cx),
                Some(Incoming::Image { kind, bytes }) => self.paste_image(kind, bytes, cx),
                None => {}
            },
        }
    }

    /// 파일 경로를 이 터미널의 셸이 읽는 모양으로 바꿔 붙여 넣는다. 뒤에 빈칸을 두어 이어서 입력하는 글자가 경로에 붙지 않게 한다.
    pub fn paste_paths(
        &mut self,
        paths: &[String],
        cx: &mut Context<Self>,
    ) {
        if !paths.is_empty() {
            match paste_path_text(paths, self.paste_place) {
                Some(text) => self.paste_text(&format!("{text} "), cx),
                // 제어 문자가 든 경로는 붙이지 않고 창이 상태 줄에 알린다.
                None => cx.emit(PasteRefused),
            }
        }
    }

    /// 경로 없는 클립보드 그림은 창이 쓰기 길로 `pasted-images`에 저장한 뒤 그 경로를 이 터미널에 붙여 넣는다(디스크 쓰기는 한 길로만 한다).
    fn paste_image(
        &mut self,
        kind: ImageKind,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        cx.emit(PasteImage { kind, bytes });
    }

    /// 끌어 고르던 선택을 끝낸다. 끌던 중이었으면 한 번만 복사한다(설정 `copy on selection`이 켜져 있을 때).
    fn finish_selection(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let was_selecting = std::mem::take(&mut self.selecting);
        let selected = if was_selecting { self.grid.term.lock().selection_to_string() } else { None };
        if let Some(text) = copy_on_selection_text(crate::look::copy_on_selection(), was_selecting, selected) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
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
        cx.emit(Typed);
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
            link: self.link.as_ref().map(|link| (link.row, link.columns.clone())),
        };
        let over_link = self.link.is_some();
        let view = cx.entity();
        let focus = self.focus.clone();
        let area = div()
            .id("terminal-area")
            .size_full()
            .px(zpx(terminal_size::PADDING_X))
            .py(zpx(terminal_size::PADDING_Y))
            .bg(hsla(default_bg()))
            .track_focus(&self.focus)
            .key_context("Terminal")
            .on_key_down(cx.listener(Self::on_key))
            // 탐색기에서 끌어 온 파일은 이 터미널에 경로로 붙여 넣는다(포커스도 이 터미널로 온다).
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, window, cx| {
                window.focus(&this.focus, cx);
                let paths: Vec<String> =
                    dropped.paths().iter().map(|path| path.to_string_lossy().to_string()).collect();
                this.paste_paths(&paths, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    // Ctrl+클릭이 파일 경로 위면 그 파일을 열고, 그 밖의 클릭은 글자 선택을 시작한다.
                    let opened = this.link.as_ref().filter(|_| event.modifiers.control).map(|link| link.target.clone());
                    if let Some(target) = opened {
                        cx.emit(OpenLink { target });
                    } else {
                        let (point, side) = this.cell_at(event.position.x, event.position.y);
                        this.grid.term.lock().selection = Some(Selection::new(SelectionType::Simple, point, side));
                        this.selecting = true;
                    }
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
                this.pointer = Some((event.position.x, event.position.y));
                this.ctrl = event.modifiers.control;
                this.refresh_link(cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                // 마우스가 터미널 밖으로 나가면 옛 위치의 밑줄·손가락 커서를 지운다.
                if !*hovered {
                    this.pointer = None;
                    this.refresh_link(cx);
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _window, cx| {
                this.ctrl = event.modifiers.control;
                this.refresh_link(cx);
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                let cell_h = this.info.borrow().cell_h.max(1.0);
                let dy = f32::from(event.delta.pixel_delta(px(cell_h)).y);
                let n_lines = (dy / cell_h).round() as i32;
                if n_lines != 0 {
                    this.grid.term.lock().scroll_display(Scroll::Delta(n_lines));
                    this.refresh_link(cx);
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    |bounds, _window, _cx| bounds,
                    move |_bounds, bounds: Bounds<Pixels>, window, cx| {
                        // 그리는 사이에 입력 방식 경로를 이 보기에 연결한다(포커스가 있을 때만 글자를 받는다).
                        window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
                        // 버튼을 어디서 떼든(터미널 밖 포함) 끌어 고르던 선택이 끝난다.
                        let ending = view.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                            if phase == gpui_kit::DispatchPhase::Bubble && event.button == MouseButton::Left {
                                ending.update(cx, |this, cx| this.finish_selection(cx));
                            }
                        });
                        paint_grid(&inputs, bounds, window, cx);
                    },
                )
                .size_full(),
            );
        if over_link { area.cursor_pointer() } else { area }
    }
}
