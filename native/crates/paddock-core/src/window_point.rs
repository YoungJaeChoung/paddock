//! 창 사이 좌표 규칙: 다른 창에서 끌던 점이 이 창의 본문 어디인지, 점이 창 밖인지.
//!
//! 창의 위치·크기를 받아 계산만 한다. 창을 만들고 마우스를 받는 일은 호출자가 맡는다.

/// 화면 전체 좌표에서 본 창 본문(그리는 영역)의 사각형.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenBounds {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// 다른 창에서 끌던 화면 좌표의 점 `screen`을 `main` 본문 기준 좌표로 바꾼다. 점이 `main` 본문 밖이면 None이다.
///
/// 제목줄 같은 창 장식은 본문 사각형에 들지 않는 것으로 본다(그리는 영역만 받는다). 분리 창을 끌던 사용자가 본 창에
/// 놓을 때, 본 창이 칸 위치와 같은 좌표계로 놓을 자리를 찾게 한다.
///
/// Examples
/// --------
/// | 본문 (left, top, w, h) | 화면 좌표  | 결과         |
/// |------------------------|------------|--------------|
/// | (100, 50, 800, 600)    | (150, 90)  | (50, 40)     |
/// | (100, 50, 800, 600)    | (50, 90)   | None (왼쪽 밖) |
/// | (100, 50, 800, 600)    | (150, 700) | None (아래 밖) |
pub fn other_window_point(
    screen: (f32, f32),
    main: ScreenBounds,
) -> Option<(f32, f32)> {
    let (x, y) = (screen.0 - main.left, screen.1 - main.top);
    if is_outside(x, y, main.width, main.height) { None } else { Some((x, y)) }
}

/// 창 안 좌표 `(x, y)`가 `width`×`height` 크기의 창 밖인지. 마우스를 누른 창은 창 밖에서 뗀 점도 음수나 창 크기를
/// 넘는 좌표로 받으므로, 이 값이 참이면 탭을 창 밖에 놓은 것이다.
///
/// Examples
/// --------
/// | (x, y)      | 창 크기   | 결과  |
/// |-------------|-----------|-------|
/// | (-1, 10)    | 800×600   | true  |
/// | (800, 10)   | 800×600   | true  |
/// | (10, 600)   | 800×600   | true  |
/// | (799, 599)  | 800×600   | false |
pub fn is_outside(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> bool {
    x < 0.0 || y < 0.0 || x >= width || y >= height
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: ScreenBounds = ScreenBounds { left: 100.0, top: 50.0, width: 800.0, height: 600.0 };

    #[test]
    fn test_td12_a_screen_point_inside_the_main_window_maps_to_its_body_coordinates() {
        assert_eq!(other_window_point((150.0, 90.0), MAIN), Some((50.0, 40.0)));
        assert_eq!(other_window_point((100.0, 50.0), MAIN), Some((0.0, 0.0)));
    }

    #[test]
    fn test_td12_a_screen_point_left_of_or_above_the_main_window_is_none() {
        assert_eq!(other_window_point((50.0, 90.0), MAIN), None);
        assert_eq!(other_window_point((150.0, 40.0), MAIN), None);
    }

    #[test]
    fn test_td12_a_screen_point_right_of_or_below_the_main_window_is_none_and_the_far_edge_is_outside() {
        assert_eq!(other_window_point((150.0, 700.0), MAIN), None);
        assert_eq!(other_window_point((900.0, 90.0), MAIN), None);
        assert_eq!(other_window_point((899.0, 649.0), MAIN), Some((799.0, 599.0)));
    }

    #[test]
    fn test_td03_a_release_at_a_negative_or_beyond_the_size_coordinate_is_outside_and_inside_points_are_not() {
        for (x, y) in [(-1.0, 10.0), (10.0, -1.0), (800.0, 10.0), (10.0, 600.0), (-300.0, 900.0)] {
            assert!(is_outside(x, y, 800.0, 600.0), "({x}, {y})");
        }
        for (x, y) in [(0.0, 0.0), (799.0, 599.0), (400.0, 300.0)] {
            assert!(!is_outside(x, y, 800.0, 600.0), "({x}, {y})");
        }
    }
}
