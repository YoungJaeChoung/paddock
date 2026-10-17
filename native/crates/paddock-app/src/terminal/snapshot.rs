//! 화면 격자에서 한 프레임을 그리는 데 필요한 셀 정보만 복사해 둔 스냅샷.
//!
//! 격자 잠금은 이 복사가 끝나는 동안만 쥔다. 글자 모양 잡기와 그리기는 잠금 없이 스냅샷으로 하므로, 데몬 출력을
//! 격자에 먹이는 스레드가 그리기 때문에 기다리지 않는다. 버퍼는 프레임 사이에 재사용해 프레임마다 셀마다 새로
//! 메모리를 잡지 않는다. 그리는 도구와 무관해 창 없이 시험할 수 있다.
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::CursorShape;
use paddock_core::cell_layout::{CellGlyph, base_span, shrink_blocked_vs16_spans};

use crate::look::colors;
use crate::terminal::palette::{cursor_bg, cursor_text, default_bg, resolve_color, selection_bg};

/// 한 셀의 글자(기본 글자와 결합 글자)에서 보관하는 최대 글자 수. 이보다 많은 결합 글자는 버린다.
pub const MAX_CELL_CHARS: usize = 8;

/// 셀 하나의 글자 모음. 메모리를 따로 잡지 않는 고정 크기라 복사와 해시가 싸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellText {
    chars: [char; MAX_CELL_CHARS],
    n_chars: u8,
}

impl CellText {
    /// 기본 글자 뒤에 결합 글자(`extra`)를 이어 붙인다.
    pub fn new(
        base: char,
        extra: Option<&[char]>,
    ) -> Self {
        let mut text = Self { chars: [' '; MAX_CELL_CHARS], n_chars: 1 };
        text.chars[0] = base;
        for c in extra.into_iter().flatten().take(MAX_CELL_CHARS - 1) {
            text.chars[text.n_chars as usize] = *c;
            text.n_chars += 1;
        }
        text
    }

    /// 글자 모양을 잡을 때 쓰는 문자열.
    pub fn to_text(self) -> String {
        self.chars[..self.n_chars as usize].iter().collect()
    }
}

/// 그릴 셀 하나의 글자·색·속성. 칸 배치는 같은 순서의 `CellGlyph`에 있다.
#[derive(Clone, Copy, Debug)]
pub struct CellStyle {
    pub text: CellText,
    pub fg: u32,
    pub flags: Flags,
}

/// 배경색을 칠할 같은 줄의 연속한 칸 묶음 (행, 시작 열, 칸 수, 색).
pub type BackgroundRun = (i32, usize, usize, u32);

/// 블록이 아닌 모양으로 그릴 커서.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorMark {
    pub shape: CursorShape,
    pub row: i32,
    pub col: usize,
    /// 커서가 차지하는 칸 수(전각 글자 위면 2).
    pub n_cells: usize,
}

/// 한 프레임의 셀 정보. 프레임마다 `fill`로 다시 채운다.
#[derive(Default)]
pub struct Snapshot {
    /// 글자를 그리거나 밑줄·취소선이 있는 셀의 칸 배치. 행 우선, 열 오름차순이다.
    pub layout: Vec<CellGlyph>,
    /// `layout`과 같은 순서의 글자·색·속성.
    pub styles: Vec<CellStyle>,
    pub backgrounds: Vec<BackgroundRun>,
    /// 블록이 아닌 모양의 보이는 커서. 블록 커서는 배경으로 칠하므로 여기에 없다.
    pub cursor_mark: Option<CursorMark>,
    /// 커서의 화면상 (행, 열). 한글 조합 미리보기를 놓는 자리이며 행은 화면 밖일 수 있다.
    pub cursor_cell: (i32, usize),
}

impl Snapshot {
    /// `term`의 보이는 화면을 `cols`x`rows` 안에서 복사한다. 이전 내용은 버린다.
    ///
    /// 블록 커서는 전각 글자 위에서 두 칸을 칠하고, 선택 강조는 전각 글자의 한쪽 칸만 선택돼도 두 칸 모두 칠한다.
    pub fn fill<T: EventListener>(
        &mut self,
        term: &Term<T>,
        cols: u16,
        rows: u16,
        focused: bool,
    ) {
        self.layout.clear();
        self.styles.clear();
        self.backgrounds.clear();
        let content = term.renderable_content();
        let display_offset = content.display_offset as i32;
        let cursor = content.cursor;
        let cursor_visible = content.mode.contains(TermMode::SHOW_CURSOR) && cursor.shape != CursorShape::Hidden;
        let selection = content.selection;
        let overrides = content.colors;

        // 커서가 전각 글자의 둘째 칸에 있다고 알려오면 첫 칸으로 옮긴다.
        let mut cursor_point = cursor.point;
        if cursor_point.column.0 > 0 && term.grid()[cursor_point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            cursor_point.column.0 -= 1;
        }
        let cursor_is_wide = term.grid()[cursor_point].flags.contains(Flags::WIDE_CHAR);
        self.cursor_cell = (cursor_point.line.0 + display_offset, cursor_point.column.0);

        for indexed in content.display_iter {
            let cell = indexed.cell;
            let row = indexed.point.line.0 + display_offset;
            let col = indexed.point.column.0;
            if row < 0 || row >= rows as i32 || col >= cols as usize {
                continue;
            }
            let mut fg = resolve_color(cell.fg, overrides, true);
            let mut bg = resolve_color(cell.bg, overrides, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if selection.as_ref().is_some_and(|selected| selected.contains_cell(&indexed, cursor.point, cursor.shape)) {
                bg = selection_bg();
                fg = colors().fg_strong;
            }
            let on_cursor = indexed.point.line == cursor_point.line
                && (indexed.point.column == cursor_point.column
                    || (cursor_is_wide && indexed.point.column.0 == cursor_point.column.0 + 1));
            if cursor_visible && focused && cursor.shape == CursorShape::Block && on_cursor {
                bg = cursor_bg();
                fg = cursor_text();
            }
            if bg != default_bg() {
                // 같은 줄에서 연속한 같은 색 배경은 한 사각형으로 합친다.
                match self.backgrounds.last_mut() {
                    Some(last) if last.0 == row && last.1 + last.2 == col && last.3 == bg => last.2 += 1,
                    _ => self.backgrounds.push((row, col, 1, bg)),
                }
            }
            let is_spacer = cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let zero_width = cell.zerowidth();
            let has_vs16 = zero_width.is_some_and(|chars| chars.contains(&'\u{fe0f}'));
            let wide = cell.flags.contains(Flags::WIDE_CHAR);
            let drawn = !is_spacer && !cell.flags.contains(Flags::HIDDEN) && !(cell.c == ' ' && zero_width.is_none());
            // 글자도 밑줄·취소선도 없는 셀은 그릴 것이 없으므로 담지 않는다.
            if drawn || cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                self.layout.push(CellGlyph {
                    row,
                    col,
                    drawn,
                    vs16: has_vs16 && !wide,
                    span: base_span(wide, has_vs16),
                });
                self.styles.push(CellStyle { text: CellText::new(cell.c, zero_width), fg, flags: cell.flags });
            }
        }
        // VS16 글자는 오른쪽 칸에 다른 글자가 있으면 1칸으로 줄여 겹치지 않게 한다.
        shrink_blocked_vs16_spans(&mut self.layout);

        self.cursor_mark = None;
        if cursor_visible
            && cursor.shape != CursorShape::Block
            && self.cursor_cell.0 >= 0
            && self.cursor_cell.0 < term.screen_lines() as i32
        {
            self.cursor_mark = Some(CursorMark {
                shape: cursor.shape,
                row: self.cursor_cell.0,
                col: self.cursor_cell.1,
                n_cells: if cursor_is_wide { 2 } else { 1 },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::index::{Column, Line, Point, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::terminal::grid::Grid;
    use crate::terminal::palette::default_fg;

    fn grid_showing(text: &str) -> Grid {
        let grid = Grid::new(10, 3, |_| {}, |_, _| {});
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *grid.term.lock(), text.as_bytes());
        grid
    }

    fn fill(grid: &Grid) -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.fill(&*grid.term.lock(), 10, 3, true);
        snapshot
    }

    #[test]
    fn test_cell_text_keeps_base_and_combining_chars_and_caps_the_rest() {
        assert_eq!(CellText::new('a', None).to_text(), "a");
        assert_eq!(CellText::new('\u{26a0}', Some(&['\u{fe0f}'])).to_text(), "\u{26a0}\u{fe0f}");
        let many = vec!['\u{301}'; 20];
        assert_eq!(CellText::new('e', Some(&many)).to_text().chars().count(), MAX_CELL_CHARS);
    }

    #[test]
    fn test_blank_cells_are_left_out_and_text_cells_keep_their_position() {
        let grid = grid_showing("ab  c");
        let snapshot = fill(&grid);
        let columns: Vec<usize> = snapshot.layout.iter().map(|cell| cell.col).collect();
        assert_eq!(columns, vec![0, 1, 4]);
        assert_eq!(snapshot.styles[2].text.to_text(), "c");
    }

    #[test]
    fn test_block_cursor_on_a_wide_char_covers_both_cells() {
        // 한글 두 글자를 쓰고 커서를 첫 글자(0, 0)로 되돌린다.
        let grid = grid_showing("한글\x1b[1;1H");
        let snapshot = fill(&grid);
        assert_eq!(snapshot.backgrounds, vec![(0, 0, 2, cursor_bg())]);
        // 커서가 놓일 글자는 배경과 같은 색으로 읽히도록 글자색이 바뀐다.
        assert_eq!(snapshot.styles[0].fg, cursor_text());
        assert_eq!(snapshot.styles[1].fg, default_fg());
    }

    #[test]
    fn test_block_cursor_on_a_narrow_char_covers_one_cell() {
        let grid = grid_showing("ab\x1b[1;1H");
        assert_eq!(fill(&grid).backgrounds, vec![(0, 0, 1, cursor_bg())]);
    }

    #[test]
    fn test_unfocused_view_draws_no_block_cursor() {
        let grid = grid_showing("ab\x1b[1;1H");
        let mut snapshot = Snapshot::default();
        snapshot.fill(&*grid.term.lock(), 10, 3, false);
        assert!(snapshot.backgrounds.is_empty());
    }

    #[test]
    fn test_selection_that_touches_only_the_second_cell_of_a_wide_char_highlights_both() {
        let grid = grid_showing("한글");
        {
            let mut term = grid.term.lock();
            let spacer = Point::new(Line(0), Column(1));
            let mut selection = Selection::new(SelectionType::Simple, spacer, Side::Left);
            selection.update(spacer, Side::Right);
            term.selection = Some(selection);
        }
        // 커서 배경이 섞이지 않도록 포커스 없는 보기로 본다.
        let mut snapshot = Snapshot::default();
        snapshot.fill(&*grid.term.lock(), 10, 3, false);
        assert_eq!(snapshot.backgrounds, vec![(0, 0, 2, selection_bg())]);
    }

    #[test]
    fn test_bar_cursor_is_kept_aside_and_cursor_cell_is_reported() {
        // DECSCUSR 6: 깜빡이지 않는 막대 커서.
        let grid = grid_showing("a\x1b[6 q");
        let snapshot = fill(&grid);
        assert!(snapshot.backgrounds.is_empty());
        assert_eq!(snapshot.cursor_mark, Some(CursorMark { shape: CursorShape::Beam, row: 0, col: 1, n_cells: 1 }));
        assert_eq!(snapshot.cursor_cell, (0, 1));
    }

    #[test]
    fn test_fill_reuses_buffers_without_keeping_old_cells() {
        let grid = grid_showing("abc");
        let mut snapshot = Snapshot::default();
        snapshot.fill(&*grid.term.lock(), 10, 3, true);
        let capacity = snapshot.layout.capacity();
        let mut parser: Processor = Processor::new();
        parser.advance(&mut *grid.term.lock(), b"\x1b[2J\x1b[Hz");
        snapshot.fill(&*grid.term.lock(), 10, 3, true);
        assert_eq!(snapshot.layout.len(), 1);
        assert!(snapshot.layout.capacity() >= capacity, "buffers are reused, not reallocated smaller");
    }
}
