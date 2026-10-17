//! 셀 격자를 GPUI로 그린다. 전각 글자는 글리프 폭을 셀 2칸에 맞춰 가운데에 놓고, 칸 수와 맞춤 계산은 core의 순수 규칙을 쓴다.
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui_kit::{
    App, Bounds, Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, ShapedLine, SharedString,
    TextAlign, TextRun, Window, fill, point, px, size,
};
use paddock_core::cell_layout::{center_offset, fit_scale};
use paddock_core::theme::{font, terminal as terminal_size, text};

use crate::look::{colors, scaled, terminal_family, terminal_font_size};
use crate::terminal::banner::banner_text;
use crate::terminal::grid::Grid;
use crate::terminal::ime::cell_span;
use crate::terminal::palette::{cursor_bg, default_bg};
use crate::terminal::snapshot::{CellText, Snapshot};

/// 터미널 글꼴 크기(px). 화면 배율을 곱한다.
fn font_px() -> f32 {
    scaled(terminal_font_size())
}

/// 글리프 캐시가 이 개수를 넘으면 비운다.
const GLYPH_CACHE_LIMIT: usize = 20_000;

/// 조합 중 글자와 밑줄 색.
fn composition_fg() -> u32 {
    colors().fg_strong
}

/// 줄·밑줄·비블록 커서의 두께(px). 화면 배율을 곱한다.
fn thickness() -> f32 {
    scaled(terminal_size::LINE_THICKNESS)
}

/// 모양을 잡아 둔 글리프의 식별값. 글자·굵기·기울임·색·칸 수가 같으면 같은 글리프다.
/// 고정 크기라 캐시 조회에 메모리를 잡지 않는다.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    text: CellText,
    bold: bool,
    italic: bool,
    color: u32,
    span: u8,
}

struct Glyph {
    line: ShapedLine,
    x_off: f32,
}

/// 보기들이 함께 쓰는 그리기 보조 자료: 모양 잡은 글리프와 프레임마다 다시 채우는 셀 스냅샷 버퍼.
#[derive(Default)]
pub struct GlyphCache {
    map: HashMap<GlyphKey, Glyph>,
    snapshot: Snapshot,
    /// `map`의 글리프를 잡은 글꼴 크기(비트)와 이름. 빠른 설정이나 배율이 바꾸면 글리프를 다시 잡는다.
    font: (u32, String),
}

/// 마지막으로 그린 격자의 위치·크기. 마우스 좌표를 셀로 바꾸는 데 쓴다.
#[derive(Clone, Copy, Default)]
pub struct PaintInfo {
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub cols: u16,
    pub rows: u16,
}

/// 격자 그리기에 필요한 것.
pub struct PaintInputs {
    pub grid: Arc<Grid>,
    pub cache: Rc<RefCell<GlyphCache>>,
    pub info: Rc<RefCell<PaintInfo>>,
    pub focused: bool,
    /// 한글 조합 중인 글자. 있으면 커서 칸부터 밑줄 친 미리보기로 겹쳐 그린다.
    pub marked: Option<String>,
}

/// 0xRRGGBB를 GPUI 색으로.
pub fn hsla(rgb: u32) -> Hsla {
    gpui_kit::rgb(rgb).into()
}

fn base_font(
    bold: bool,
    italic: bool,
) -> Font {
    Font {
        family: terminal_family().into(),
        features: FontFeatures::default(),
        fallbacks: Some(FontFallbacks::from_fonts(vec![font::TERMINAL_FALLBACK_FAMILY.to_string()])),
        weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
        style: if italic { FontStyle::Italic } else { FontStyle::Normal },
    }
}

fn shape(
    window: &Window,
    text: &str,
    font_size: Pixels,
    bold: bool,
    italic: bool,
    color: u32,
) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: base_font(bold, italic),
        color: hsla(color),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(text.to_string()), font_size, &[run], None)
}

/// 격자를 `bounds` 안에 그린다. 그리기 전에 창 크기에 맞는 열·행으로 격자와 PTY를 맞춘다.
///
/// 격자 잠금은 셀 정보를 스냅샷으로 복사하는 동안만 쥔다. 글자 모양 잡기와 그리기는 잠금 없이 한다.
pub fn paint_grid(
    inputs: &PaintInputs,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let text_system = window.text_system().clone();
    let font_size = px(font_px());
    let font_id = text_system.resolve_font(&base_font(false, false));
    // 셀 폭은 정수 px로 맞춰 열 경계가 흔들리지 않게 한다.
    let cell_w = f32::from(
        text_system.advance(font_id, font_size, 'M').map(|advance| advance.width).unwrap_or(px(font_px() * 0.55)),
    )
    .round()
    .max(1.0);
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    let descent = f32::from(text_system.descent(font_id, font_size)).abs();
    // 칸 높이는 글꼴 자체 줄 높이의 배수다(줄 높이 배수가 1보다 크면 위아래에 여백이 생긴다).
    let cell_h = ((ascent + descent).round() * text::TERMINAL_LINE_HEIGHT).floor();
    let baseline_offset = (cell_h - (ascent + descent)) / 2.0 + ascent;

    let cols = ((f32::from(bounds.size.width) / cell_w).floor() as u16).max(2);
    let rows = ((f32::from(bounds.size.height) / cell_h).floor() as u16).max(1);
    let origin_x = f32::from(bounds.origin.x);
    let origin_y = f32::from(bounds.origin.y);
    *inputs.info.borrow_mut() = PaintInfo { origin_x, origin_y, cell_w, cell_h, cols, rows };
    inputs.grid.resize(cols, rows, cell_w as u16, cell_h as u16);

    window.paint_quad(fill(bounds, hsla(default_bg())));

    let mut cache = inputs.cache.borrow_mut();
    let GlyphCache { map, snapshot, font: shaped_with } = &mut *cache;
    let current_font = (font_px().to_bits(), terminal_family());
    if *shaped_with != current_font {
        map.clear();
        *shaped_with = current_font;
    }
    snapshot.fill(&*inputs.grid.term.lock(), cols, rows, inputs.focused);
    if map.len() > GLYPH_CACHE_LIMIT {
        map.clear();
    }

    for (row, col, n_cells, bg) in &snapshot.backgrounds {
        let quad = Bounds::new(
            point(px(origin_x + *col as f32 * cell_w), px(origin_y + *row as f32 * cell_h)),
            size(px(*n_cells as f32 * cell_w), px(cell_h)),
        );
        window.paint_quad(fill(quad, hsla(*bg)));
    }

    for (cell, style) in snapshot.layout.iter().zip(&snapshot.styles) {
        let cell_x = origin_x + cell.col as f32 * cell_w;
        let cell_y = origin_y + cell.row as f32 * cell_h;
        if cell.drawn {
            let bold = style.flags.contains(Flags::BOLD);
            let italic = style.flags.contains(Flags::ITALIC);
            let key = GlyphKey { text: style.text, bold, italic, color: style.fg, span: cell.span as u8 };
            let glyph = map.entry(key).or_insert_with(|| {
                let text = style.text.to_text();
                let mut line = shape(window, &text, font_size, bold, italic, style.fg);
                // 전각(2칸)은 글리프 폭을 셀 2칸에 맞추고, 1칸이 넘치는 대체 글꼴 글리프는 1칸에 맞춰 줄인다.
                let scale = fit_scale(f32::from(line.width()), cell.span, cell_w);
                if scale != 1.0 {
                    line = shape(window, &text, px(font_px() * scale), bold, italic, style.fg);
                }
                let x_off = center_offset(cell.span, cell_w, f32::from(line.width()));
                Glyph { line, x_off }
            });
            let glyph_ascent = f32::from(glyph.line.ascent);
            let glyph_descent = f32::from(glyph.line.descent).abs();
            // 글꼴마다 달라지는 기준선을 셀 기준선에 맞춘다.
            let y = cell_y + baseline_offset - ((cell_h - glyph_ascent - glyph_descent) / 2.0 + glyph_ascent);
            let _ =
                glyph.line.paint(point(px(cell_x + glyph.x_off), px(y)), px(cell_h), TextAlign::Left, None, window, cx);
        }
        let span_w = cell.span as f32 * cell_w;
        if style.flags.intersects(Flags::ALL_UNDERLINES) {
            let line = Bounds::new(
                point(px(cell_x), px(cell_y + cell_h - 2.0 * thickness())),
                size(px(span_w), px(thickness())),
            );
            window.paint_quad(fill(line, hsla(style.fg)));
        }
        if style.flags.contains(Flags::STRIKEOUT) {
            let line = Bounds::new(point(px(cell_x), px(cell_y + cell_h / 2.0)), size(px(span_w), px(thickness())));
            window.paint_quad(fill(line, hsla(style.fg)));
        }
    }

    // 블록이 아닌 커서 모양.
    if let Some(mark) = snapshot.cursor_mark {
        let x = origin_x + mark.col as f32 * cell_w;
        let y = origin_y + mark.row as f32 * cell_h;
        let width = mark.n_cells as f32 * cell_w;
        let bar = match mark.shape {
            CursorShape::Beam => {
                Bounds::new(point(px(x), px(y)), size(px(scaled(terminal_size::BEAM_WIDTH)), px(cell_h)))
            }
            CursorShape::Underline => {
                Bounds::new(point(px(x), px(y + cell_h - 2.0 * thickness())), size(px(width), px(2.0 * thickness())))
            }
            _ => Bounds::new(point(px(x), px(y)), size(px(width), px(thickness()))),
        };
        window.paint_quad(fill(bar, hsla(cursor_bg())));
    }
    let (cursor_row, cursor_col) = snapshot.cursor_cell;
    if let Some(marked) = inputs.marked.as_deref()
        && cursor_row >= 0
        && cursor_row < rows as i32
    {
        paint_composition(window, cx, marked, (cursor_row, cursor_col), (cols, cell_w, cell_h), (origin_x, origin_y));
    }
    drop(cache);

    // 안내 띠는 오른쪽 위에 띄운다.
    if let Some(text) = banner_text(&inputs.grid.shared) {
        let line = shape(window, &text, px(scaled(text::SMALL)), false, false, colors().terminal_bg);
        let (margin, pad_x, pad_y) = (
            scaled(terminal_size::BANNER_MARGIN),
            scaled(terminal_size::BANNER_PADDING_X),
            scaled(terminal_size::BANNER_PADDING_Y),
        );
        let width = f32::from(line.width()) + 2.0 * pad_x;
        let strip = Bounds::new(
            point(px(origin_x + cols as f32 * cell_w - width - margin), px(origin_y + margin)),
            size(px(width), px(cell_h + 2.0 * pad_y)),
        );
        window.paint_quad(fill(strip, hsla(colors().warning)));
        let _ = line.paint(
            point(strip.origin.x + px(pad_x), strip.origin.y + px(pad_y)),
            px(cell_h),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

/// 조합 중인 글자를 커서 칸부터 그린다. 아래 칸 내용을 가리는 바탕 위에 글자를 칸 폭에 맞춰 놓고 밑줄을 긋는다.
/// 줄 끝을 넘는 글자는 그리지 않는다.
fn paint_composition(
    window: &mut Window,
    cx: &mut App,
    marked: &str,
    (row, start_col): (i32, usize),
    (cols, cell_w, cell_h): (u16, f32, f32),
    (origin_x, origin_y): (f32, f32),
) {
    let font_size = px(font_px());
    let y = origin_y + row as f32 * cell_h;
    let mut col = start_col;
    for c in marked.chars() {
        let span = cell_span(c);
        if col + span > cols as usize {
            break;
        }
        let x = origin_x + col as f32 * cell_w;
        let area = Bounds::new(point(px(x), px(y)), size(px(span as f32 * cell_w), px(cell_h)));
        window.paint_quad(fill(area, hsla(default_bg())));
        let text = c.to_string();
        let mut line = shape(window, &text, font_size, false, false, composition_fg());
        let scale = fit_scale(f32::from(line.width()), span, cell_w);
        if scale != 1.0 {
            line = shape(window, &text, px(font_px() * scale), false, false, composition_fg());
        }
        let x_off = center_offset(span, cell_w, f32::from(line.width()));
        let glyph_height = f32::from(line.ascent) + f32::from(line.descent).abs();
        let glyph_y = y + (cell_h - glyph_height) / 2.0;
        let _ = line.paint(point(px(x + x_off), px(glyph_y)), px(cell_h), TextAlign::Left, None, window, cx);
        let underline = Bounds::new(
            point(px(x), px(y + cell_h - 2.0 * thickness())),
            size(px(span as f32 * cell_w), px(thickness())),
        );
        window.paint_quad(fill(underline, hsla(composition_fg())));
        col += span;
    }
}
