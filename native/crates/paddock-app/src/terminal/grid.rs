//! 화면 격자(`Term`)와 그 입력·크기 변경 통로. 데몬에서 받은 바이트를 앱이 격자에 먹이고, 키 입력과 크기 변경은 데몬으로 보낸다.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Processor, Rgb};
use paddock_protocol::message::TerminalState;

use crate::terminal::palette::{cursor_bg, default_bg, default_fg, indexed_rgb};

/// 터미널마다 보관하는 스크롤백 줄 수.
pub const SCROLLBACK_LINES: usize = 1000;

/// 셀 크기를 아직 모를 때 터미널 프로그램에 알리는 값(px). 첫 그리기 뒤 실제 값으로 바뀐다.
const INITIAL_CELL_SIZE: (u16, u16) = (8, 16);

/// 격자 크기(열·행)를 `Term`에 알리는 값.
struct GridSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// 입력 바이트를 데몬으로 보내는 함수.
type InputFn = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// 받는 스레드·그리는 스레드·입력 쪽이 함께 보는 격자 상태 표시.
#[derive(Default)]
pub struct Shared {
    /// 격자가 바뀌어 다시 그려야 한다.
    pub dirty: AtomicBool,
    /// 재생 중에는 격자가 만드는 응답(커서 위치 질의의 답 등)을 셸에 쓰지 않는다. 지난 질의에 다시 답하면 입력이 오염된다.
    pub replaying: AtomicBool,
    /// 셸이 끝났다.
    pub exited: AtomicBool,
    /// 데몬과의 연결이 끊겼다(셸이 끝난 경우 포함).
    pub disconnected: AtomicBool,
    /// 대체 화면(vim 등)이 켜진 채 재생을 마쳤다는 표시. 입력이 들어오면 내린다.
    pub replay_notice: AtomicBool,
    /// 다른 창(연결)이 이 터미널의 출력을 이어받아 이 연결로는 더 오지 않는다.
    pub detached: AtomicBool,
    /// 아직 답하지 못한 색 질의(번호와 응답 글자 만드는 함수). 격자를 처리하는 도중에는 격자 잠금을 쥐고 있어
    /// 프로그램이 바꾼 팔레트를 읽을 수 없으므로, 처리가 끝난 뒤 `Grid::answer_color_requests`가 답한다.
    color_requests: Mutex<Vec<(usize, ColorReply)>>,
    /// 셸이 끝났을 때 데몬이 알려 준 종료 코드 글자(코드를 모르면 `unknown`). 끝나지 않았으면 None이다.
    pub exit_code: Mutex<Option<String>>,
    /// 데몬이 알린 이 터미널의 폴더·앞쪽 프로그램·에이전트 활동. 아직 받지 못했으면 None이다.
    /// `finished`는 알림 사건이므로 여기에는 늘 거짓으로 남고, 사건은 `finished_pending`이 센다.
    pub state: Mutex<Option<TerminalState>>,
    /// `state`가 바뀔 때마다 오르는 번호. 창은 이 번호가 달라졌을 때만 목록을 다시 그린다.
    pub state_version: AtomicU32,
    /// 아직 처리하지 않은 완료 알림 수. 창이 꺼내 가며(0으로 되돌리며) 미확인 표시·소리를 정한다.
    pub finished_pending: AtomicU32,
    pub title: Mutex<String>,
    /// 데몬에서 출력(재생 포함)을 마지막으로 받은 시각. 아직 받지 않았으면 None이다. 되살린 터미널이 프롬프트를 낸 때를 가리는 데 쓴다.
    pub last_output_at: Mutex<Option<std::time::Instant>>,
    input: Mutex<Option<InputFn>>,
    /// 마지막으로 알린 (열, 행, 셀 너비 px, 셀 높이 px).
    size: Mutex<(u16, u16, u16, u16)>,
}

/// 색 질의 응답을 글자로 만드는 함수(색을 받아 응답 글자를 돌려준다).
type ColorReply = Arc<dyn Fn(Rgb) -> String + Sync + Send + 'static>;

/// 프로그램이 색 질의(OSC 4·10·11·12)로 물은 번호의 앱 기본 색. 256은 글자색, 257은 배경색, 258은 커서색이다.
/// 프로그램이 그 번호의 색을 바꿔 두지 않았을 때의 답이다. 알 수 없는 번호는 글자색이다.
fn default_palette_rgb(index: usize) -> Rgb {
    let value = match index {
        0..=255 => indexed_rgb(index as u8),
        257 => default_bg(),
        258 => cursor_bg(),
        _ => default_fg(),
    };
    Rgb { r: (value >> 16) as u8, g: (value >> 8) as u8, b: value as u8 }
}

/// 격자가 만드는 이벤트(다시 그리기 요청, 제목, 터미널 질의의 응답, 종료)를 `Shared`에 반영한다.
#[derive(Clone)]
pub struct Proxy(pub Arc<Shared>);

impl EventListener for Proxy {
    fn send_event(
        &self,
        event: Event,
    ) {
        let shared = &self.0;
        match event {
            Event::Wakeup => shared.dirty.store(true, Ordering::Relaxed),
            Event::Title(title) => *shared.title.lock().unwrap() = title,
            Event::PtyWrite(text) => {
                if !shared.replaying.load(Ordering::Relaxed)
                    && let Some(input) = shared.input.lock().unwrap().as_ref()
                {
                    input(text.into_bytes());
                }
            }
            Event::ColorRequest(index, format) => {
                if !shared.replaying.load(Ordering::Relaxed) {
                    shared.color_requests.lock().unwrap().push((index, format));
                }
            }
            Event::TextAreaSizeRequest(format) => {
                if !shared.replaying.load(Ordering::Relaxed) {
                    let (cols, rows, cell_width, cell_height) = *shared.size.lock().unwrap();
                    let text = format(WindowSize { num_lines: rows, num_cols: cols, cell_width, cell_height });
                    if let Some(input) = shared.input.lock().unwrap().as_ref() {
                        input(text.into_bytes());
                    }
                }
            }
            Event::Exit | Event::ChildExit(_) => {
                shared.exited.store(true, Ordering::Relaxed);
                shared.dirty.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

/// 터미널 하나의 화면 격자와 그 입력·크기 통로.
pub struct Grid {
    pub term: Arc<FairMutex<Term<Proxy>>>,
    pub shared: Arc<Shared>,
    resize_pty: Box<dyn Fn(u16, u16) + Send + Sync>,
}

impl Grid {
    /// `cols`x`rows` 격자를 만든다. `input`은 입력 바이트를 데몬으로 보내고, `resize_pty`는 새 열·행을 데몬에 알린다.
    pub fn new(
        cols: u16,
        rows: u16,
        input: impl Fn(Vec<u8>) + Send + Sync + 'static,
        resize_pty: impl Fn(u16, u16) + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        *shared.size.lock().unwrap() = (cols, rows, INITIAL_CELL_SIZE.0, INITIAL_CELL_SIZE.1);
        *shared.input.lock().unwrap() = Some(Arc::new(input));
        let config = Config { scrolling_history: SCROLLBACK_LINES, ..Config::default() };
        let term = Term::new(config, &GridSize { cols: cols as usize, rows: rows as usize }, Proxy(shared.clone()));
        Self { term: Arc::new(FairMutex::new(term)), shared, resize_pty: Box::new(resize_pty) }
    }

    /// 사용자 입력을 데몬으로 보낸다. 입력이 있으면 재생 뒤 안내(`replay_notice`)와 선택은 사라진다.
    pub fn input(
        &self,
        bytes: Vec<u8>,
    ) {
        self.shared.replay_notice.store(false, Ordering::Relaxed);
        // 입력하면 선택은 풀린다(xterm.js와 같다). 그래야 선택이 남은 채로 명령을 실행해도 다음 Ctrl+C가 복사가 아닌 셸의 ^C가 된다.
        self.term.lock().selection = None;
        if let Some(input) = self.shared.input.lock().unwrap().as_ref() {
            input(bytes);
        }
    }

    /// 격자와 PTY를 같은 열·행으로 맞춘다. 같은 크기거나 재생 중이면 아무것도 하지 않고 false다.
    ///
    /// 재생 바이트는 데몬이 가진 크기를 기준으로 만든 화면이라, 재생 도중 크기를 바꾸면 줄이 다른 폭으로 접혀 화면이 어긋난다.
    /// 재생이 끝나면 화면을 다시 그리므로(`dirty`) 그때 그리기가 크기를 맞춘다.
    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> bool {
        let replaying = self.shared.replaying.load(Ordering::Relaxed);
        let changed = !replaying && {
            let mut term = self.term.lock();
            let changed = term.columns() != cols as usize || term.screen_lines() != rows as usize;
            if changed {
                term.resize(GridSize { cols: cols as usize, rows: rows as usize });
            }
            changed
        };
        if changed {
            *self.shared.size.lock().unwrap() = (cols, rows, cell_width, cell_height);
            (self.resize_pty)(cols, rows);
        }
        changed
    }

    /// 쌓인 색 질의에 답한다. 프로그램이 그 번호의 색을 바꿔 두었으면 그 색을, 아니면 앱 기본 색을 셸에 쓴다.
    ///
    /// 격자에 바이트를 먹인 직후(격자 잠금을 놓은 뒤)에 부른다. 답할 질의가 없으면 아무 일도 하지 않는다.
    pub fn answer_color_requests(&self) {
        let requests = std::mem::take(&mut *self.shared.color_requests.lock().unwrap());
        for (index, format) in requests {
            let changed = self.term.lock().colors()[index];
            let text = format(changed.unwrap_or_else(|| default_palette_rgb(index)));
            if let Some(input) = self.shared.input.lock().unwrap().as_ref() {
                input(text.into_bytes());
            }
        }
    }

    /// 화면과 스크롤백을 비운다. 데몬이 화면 전체를 다시 보낼 때(`RESYNC`) 지금까지 쌓은 화면을 버리는 데 쓴다.
    pub fn reset_screen(&self) {
        // 터미널 초기화 시퀀스(RIS)는 화면·스크롤백·모드·선택을 처음 상태로 돌린다.
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *self.term.lock(), b"\x1bc");
        self.shared.dirty.store(true, Ordering::Relaxed);
    }

    /// 보이는 화면을 줄 단위 글자로 돌려준다. 전각 글자의 둘째 칸은 건너뛰고 줄 끝 공백은 자른다.
    ///
    /// Examples
    /// --------
    /// | 화면 내용      | 결과(첫 줄)  |
    /// |----------------|--------------|
    /// | `한글 ab` + 공백 | `"한글 ab"`  |
    pub fn visible_text(&self) -> Vec<String> {
        let term = self.term.lock();
        (0..term.screen_lines()).map(|row| line_text(&term, Line(row as i32))).collect()
    }

    /// 화면 위로 밀려난 줄(스크롤백)의 글자. 오래된 줄이 먼저다.
    pub fn history_text(&self) -> Vec<String> {
        let term = self.term.lock();
        (1..=term.history_size()).rev().map(|back| line_text(&term, Line(-(back as i32)))).collect()
    }
}

/// 격자의 한 줄을 글자로 만든다. 넓은 글자의 빈 칸은 건너뛰고 줄 끝 공백은 뗀다.
fn line_text<T>(
    term: &Term<T>,
    line: Line,
) -> String {
    let grid = term.grid();
    let mut text = String::new();
    for col in 0..term.columns() {
        let cell = &grid[line][Column(col)];
        if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            text.push(cell.c);
            for extra in cell.zerowidth().into_iter().flatten() {
                text.push(*extra);
            }
        }
    }
    text.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use alacritty_terminal::index::{Point as GridPoint, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};

    use super::*;

    #[test]
    fn test_user_input_clears_the_selection_but_a_copy_does_not_go_through_input() {
        let grid = Grid::new(10, 3, |_| {}, |_, _| {});
        let point = |column| GridPoint::new(Line(0), Column(column));
        let select = |grid: &Grid| {
            let mut selection = Selection::new(SelectionType::Simple, point(0), Side::Left);
            selection.update(point(2), Side::Right);
            grid.term.lock().selection = Some(selection);
        };
        select(&grid);
        assert!(grid.term.lock().selection.is_some());
        // 글자를 치거나 붙여넣거나 IME로 확정한 입력은 모두 `input`을 지나 선택을 푼다 — 그래야 다음 Ctrl+C가 셸의 ^C가 된다.
        grid.input(b"x".to_vec());
        assert!(grid.term.lock().selection.is_none());
    }

    #[test]
    fn test_resize_updates_grid_and_asks_pty_once() {
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let recorded = resizes.clone();
        let grid = Grid::new(80, 24, |_| {}, move |cols, rows| recorded.lock().unwrap().push((cols, rows)));
        assert!(grid.resize(100, 30, 8, 16));
        let (cols, rows) = {
            let term = grid.term.lock();
            (term.columns(), term.screen_lines())
        };
        assert_eq!((cols, rows), (100, 30));
        // 같은 크기를 다시 알려도 PTY에 다시 알리지 않는다.
        assert!(!grid.resize(100, 30, 8, 16));
        assert_eq!(*resizes.lock().unwrap(), vec![(100, 30)]);
    }

    #[test]
    fn test_history_text_holds_the_lines_pushed_above_the_screen_oldest_first() {
        let grid = Grid::new(10, 2, |_| {}, |_, _| {});
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *grid.term.lock(), b"one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(grid.visible_text(), vec!["three".to_string(), "four".to_string()]);
        assert_eq!(grid.history_text(), vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn test_visible_text_skips_wide_char_spacers_and_trims_lines() {
        let grid = Grid::new(10, 3, |_| {}, |_, _| {});
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *grid.term.lock(), "한글 ab".as_bytes());
        assert_eq!(grid.visible_text(), vec!["한글 ab".to_string(), String::new(), String::new()]);
    }

    fn grid_recording_input() -> (Grid, Arc<Mutex<Vec<u8>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let recorded = sent.clone();
        let grid = Grid::new(20, 5, move |bytes| recorded.lock().unwrap().extend(bytes), |_, _| {});
        (grid, sent)
    }

    fn feed(
        grid: &Grid,
        text: &str,
    ) {
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *grid.term.lock(), text.as_bytes());
    }

    #[test]
    fn test_resize_waits_while_replaying_and_applies_after() {
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let recorded = resizes.clone();
        let grid = Grid::new(80, 24, |_| {}, move |cols, rows| recorded.lock().unwrap().push((cols, rows)));
        grid.shared.replaying.store(true, Ordering::Relaxed);
        // 재생 중에 줄이면 재생 바이트가 만든 화면이 다른 폭으로 접힌다. 재생이 끝날 때까지 크기를 바꾸지 않는다.
        assert!(!grid.resize(100, 30, 8, 16));
        assert_eq!(grid.term.lock().columns(), 80);
        assert!(resizes.lock().unwrap().is_empty());
        grid.shared.replaying.store(false, Ordering::Relaxed);
        assert!(grid.resize(100, 30, 8, 16));
        assert_eq!(*resizes.lock().unwrap(), vec![(100, 30)]);
    }

    /// 0xRRGGBB를 OSC 응답의 `rgb:RRRR/GGGG/BBBB` 꼴로.
    fn osc_rgb(value: u32) -> String {
        let (r, g, b) = ((value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff);
        format!("rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}")
    }

    #[test]
    fn test_color_query_answers_with_palette_colors_but_not_while_replaying() {
        let (grid, sent) = grid_recording_input();
        feed(&grid, "\x1b]11;?\x07");
        grid.answer_color_requests();
        let background = format!("\x1b]11;{}\x07", osc_rgb(default_bg()));
        assert_eq!(String::from_utf8(sent.lock().unwrap().clone()).unwrap(), background);
        sent.lock().unwrap().clear();
        feed(&grid, "\x1b]10;?\x1b\\");
        grid.answer_color_requests();
        let foreground = format!("\x1b]10;{}\x1b\\", osc_rgb(default_fg()));
        assert_eq!(String::from_utf8(sent.lock().unwrap().clone()).unwrap(), foreground);
        sent.lock().unwrap().clear();
        grid.shared.replaying.store(true, Ordering::Relaxed);
        feed(&grid, "\x1b]11;?\x07");
        grid.answer_color_requests();
        assert!(sent.lock().unwrap().is_empty(), "queries replayed from the past get no answer");
    }

    #[test]
    fn test_color_query_answers_with_the_color_the_program_set() {
        // 프로그램이 팔레트 1번과 배경색을 바꿔 두면, 이후 질의는 앱 기본 색이 아니라 그 색으로 답한다.
        let (grid, sent) = grid_recording_input();
        feed(&grid, "\x1b]4;1;rgb:12/34/56\x07\x1b]11;rgb:ab/cd/ef\x07");
        feed(&grid, "\x1b]4;1;?\x07\x1b]11;?\x07");
        grid.answer_color_requests();
        let answered = String::from_utf8(sent.lock().unwrap().clone()).unwrap();
        assert!(answered.contains("\x1b]4;1;rgb:1212/3434/5656\x07"), "{answered:?}");
        assert!(answered.contains("\x1b]11;rgb:abab/cdcd/efef\x07"), "{answered:?}");
    }

    #[test]
    fn test_clear_screen_state_forgets_screen_and_scrollback() {
        let grid = Grid::new(10, 3, |_| {}, |_, _| {});
        feed(&grid, "a\r\nb\r\nc\r\nd\r\ne");
        assert!(grid.term.lock().history_size() > 0);
        grid.reset_screen();
        assert_eq!(grid.visible_text(), vec![String::new(), String::new(), String::new()]);
        assert_eq!(grid.term.lock().history_size(), 0);
    }
}
