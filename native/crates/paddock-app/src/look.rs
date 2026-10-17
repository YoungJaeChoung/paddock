//! 이 실행에서 쓰는 화면 모양: 색 모드, 화면 배율, 터미널·창 틀 글꼴.
//!
//! 설정에서 처음 쓸 때 한 번 읽고, 빠른 설정이 값을 바꾸면 `reload`로 다시 읽어 모든 보기가 다음 그리기부터 새 값을 쓴다.
//! 색·크기 값 자체는 core의 `theme`가 정하고, 여기서는 그중 무엇을 쓸지(다크·라이트, 배율, 글꼴)만 정한다.
use std::sync::{LazyLock, RwLock};

use paddock_core::settings_text::Appearance;
use paddock_core::theme::{Colors, Mode, font, zoom_factor};

use crate::settings::Settings;

static APPEARANCE: LazyLock<RwLock<Appearance>> = LazyLock::new(|| RwLock::new(Settings::load().appearance()));

/// 지금 쓰는 모양 설정에서 값 하나를 꺼낸다.
fn read<T>(pick: impl FnOnce(&Appearance) -> T) -> T {
    pick(&APPEARANCE.read().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

/// 설정 파일을 다시 읽어 모양 설정을 바꾼다.
pub fn reload() {
    let appearance = Settings::load().appearance();
    *APPEARANCE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = appearance;
}

/// 지금의 색 모드.
pub fn mode() -> Mode {
    read(|appearance| appearance.mode)
}

/// 지금의 색 묶음.
pub fn colors() -> &'static Colors {
    mode().colors()
}

/// 지금의 화면 배율(100%가 1.0). 모든 크기(px)에 곱한다.
pub fn zoom() -> f32 {
    read(|appearance| zoom_factor(appearance.zoom_level))
}

/// 100% 배율의 크기 `size`를 지금의 배율로 키운 값.
pub fn scaled(size: f32) -> f32 {
    size * zoom()
}

/// 터미널 글꼴 크기(100% 배율의 px).
pub fn terminal_font_size() -> f32 {
    read(|appearance| appearance.terminal_font_size as f32)
}

/// 터미널 주 글꼴 이름. 설정이 없으면 플랫폼 기본이다.
pub fn terminal_family() -> String {
    read(|appearance| appearance.terminal_font_family.clone()).unwrap_or_else(|| font::TERMINAL_FAMILY.to_string())
}

/// 창 틀 글꼴 이름. 설정이 없으면 플랫폼 기본이다.
pub fn interface_family() -> String {
    read(|appearance| appearance.interface_font_family.clone()).unwrap_or_else(|| font::UI_FAMILY.to_string())
}
