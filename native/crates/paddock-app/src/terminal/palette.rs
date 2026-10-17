//! 터미널 색 규칙: 색 지정(이름·번호·직접 지정)을 0xRRGGBB 값으로 바꾼다. 그리기 도구와 무관한 순수 계산이다.
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color as TColor, NamedColor, Rgb};

use crate::look::colors;

/// 터미널 기본 글자색.
pub fn default_fg() -> u32 {
    colors().terminal_fg
}

/// 터미널 기본 배경색.
pub fn default_bg() -> u32 {
    colors().terminal_bg
}

/// 선택 영역 배경색.
pub fn selection_bg() -> u32 {
    colors().terminal_selection
}

/// 커서(블록)의 배경색.
pub fn cursor_bg() -> u32 {
    colors().terminal_cursor
}

/// 블록 커서 위에 놓이는 글자색.
pub fn cursor_text() -> u32 {
    colors().terminal_cursor_text
}

/// `NamedColor`의 Dim 계열(일반색 0..7을 어둡게)이 차지하는 번호 범위.
const DIM_RANGE: std::ops::RangeInclusive<usize> = 259..=266;

/// 0..255 번호색의 값. 0..15는 ANSI 16색, 16..231은 6x6x6 색 큐브, 232..255는 회색 단계다.
pub fn indexed_rgb(index: u8) -> u32 {
    match index {
        0..=15 => colors().terminal_ansi[index as usize],
        16..=231 => {
            let n = index as u32 - 16;
            let level = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
            (level(n / 36) << 16) | (level((n / 6) % 6) << 8) | level(n % 6)
        }
        _ => {
            let v = 8 + (index as u32 - 232) * 10;
            (v << 16) | (v << 8) | v
        }
    }
}

/// 셀의 색 지정 하나를 값으로 바꾼다. 프로그램이 팔레트를 바꿨으면(`overrides`) 그 값이 우선한다.
///
/// `is_fg`는 이름 없는 대체 색의 기본값을 글자색과 배경색 중 무엇으로 할지 고른다.
pub fn resolve_color(
    color: TColor,
    overrides: &Colors,
    is_fg: bool,
) -> u32 {
    let to_u32 = |rgb: Rgb| ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32;
    match color {
        TColor::Spec(rgb) => to_u32(rgb),
        TColor::Indexed(index) => overrides[index as usize].map(to_u32).unwrap_or_else(|| indexed_rgb(index)),
        TColor::Named(named) => {
            let index = named as usize;
            if let Some(rgb) = overrides[index] {
                to_u32(rgb)
            } else {
                match named {
                    NamedColor::Foreground | NamedColor::BrightForeground => default_fg(),
                    NamedColor::Background => default_bg(),
                    NamedColor::Cursor => cursor_bg(),
                    _ if index < 16 => colors().terminal_ansi[index],
                    _ if DIM_RANGE.contains(&index) => {
                        let base = colors().terminal_ansi[index - 259];
                        let dim = |v: u32| v * 2 / 3;
                        (dim((base >> 16) & 0xff) << 16) | (dim((base >> 8) & 0xff) << 8) | dim(base & 0xff)
                    }
                    _ if is_fg => default_fg(),
                    _ => default_bg(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::term::color::Colors;

    use super::*;

    #[test]
    fn test_indexed_rgb_covers_ansi_cube_and_gray_ramp() {
        assert_eq!(indexed_rgb(1), 0xcd3131);
        assert_eq!(indexed_rgb(16), 0x000000);
        assert_eq!(indexed_rgb(21), 0x0000ff);
        assert_eq!(indexed_rgb(196), 0xff0000);
        assert_eq!(indexed_rgb(232), 0x080808);
        assert_eq!(indexed_rgb(255), 0xeeeeee);
    }

    #[test]
    fn test_resolve_color_uses_defaults_and_overrides() {
        let mut colors = Colors::default();
        assert_eq!(resolve_color(TColor::Named(NamedColor::Foreground), &colors, true), default_fg());
        assert_eq!(resolve_color(TColor::Named(NamedColor::Background), &colors, false), default_bg());
        assert_eq!(resolve_color(TColor::Spec(Rgb { r: 1, g: 2, b: 3 }), &colors, true), 0x010203);
        // 프로그램이 팔레트를 바꿨으면 그 값이 우선한다.
        colors[1] = Some(Rgb { r: 9, g: 8, b: 7 });
        assert_eq!(resolve_color(TColor::Indexed(1), &colors, true), 0x090807);
    }

    #[test]
    fn test_dim_colors_are_two_thirds_of_the_normal_color() {
        let colors = Colors::default();
        // 일반 빨강 0xcd3131의 2/3(소수점 아래 버림).
        assert_eq!(resolve_color(TColor::Named(NamedColor::DimRed), &colors, true), 0x882020);
    }
}
