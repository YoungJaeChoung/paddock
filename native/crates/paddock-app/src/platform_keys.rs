//! 운영체제 키보드·입력기 호출: 단축키 판정에 쓰는 영문 키 이름, 입력기 조합 끝내기.
//!
//! GPUI는 키 이름을 현재 배열에서 그 키가 내는 글자로 넘긴다. 러시아어 배열의 Ctrl+B는 `И`로 와서 영문 이름으로 정한
//! 앱 단축키와 맞지 않으므로, 같은 키 자리의 영문 이름을 여기서 구한다. 한글 두벌식은 배열 자체가 라틴 글자를 내므로 바뀌지 않는다.
#[cfg(windows)]
use std::collections::HashMap;

#[cfg(windows)]
use paddock_core::keyboard_layout::latin_equivalent_key;

/// 단축키 판정에 쓸 키 이름. 비라틴 배열의 글자 키는 같은 자리의 영문 소문자로, 그 밖의 키는 받은 이름 그대로다.
///
/// Examples
/// --------
/// | 배열     | 받은 키 | 결과  |
/// |----------|---------|-------|
/// | 러시아어 | `И`     | `b`   |
/// | 미국     | `b`     | `b`   |
/// | 어느 배열 | `left` | `left` |
#[cfg(windows)]
pub fn shortcut_key(key: &str) -> String {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_CHAR, MapVirtualKeyW};

    let equivalent = if key.is_ascii() {
        None
    } else {
        // 글자 키 26개가 이 배열에서 내는 글자. 가상 키 코드는 배열과 무관하게 `A`~`Z`의 글자 값이다.
        let mapping: HashMap<String, String> = ('A'..='Z')
            .filter_map(|letter| {
                // SAFETY: 인자는 값 두 개뿐이고 포인터를 받지 않는다.
                let code = unsafe { MapVirtualKeyW(letter as u32, MAPVK_VK_TO_CHAR) } & 0xFFFF;
                char::from_u32(code)
                    .filter(|character| *character != '\0')
                    .map(|character| (format!("Key{letter}"), character.to_string()))
            })
            .collect();
        latin_equivalent_key(&mapping, key)
    };
    equivalent.unwrap_or_else(|| key.to_string())
}

/// 단축키 판정에 쓸 키 이름. Windows가 아니면 받은 이름 그대로다.
#[cfg(not(windows))]
pub fn shortcut_key(key: &str) -> String {
    key.to_string()
}

/// 입력기에서 조합 중인 글자를 지금 확정하게 한다. 확정된 글자는 입력기가 평소 확정 때처럼 창의 입력 처리기로 보낸다.
///
/// 조합이 없거나 Windows가 아니면 아무 일도 하지 않는다. 창 앞에서 포커스가 다른 곳으로 옮겨 가기 전에 불러, 조합 중이던
/// 글자가 새 입력칸에 덮여 들어가지 않고 원래 입력 대상에 들어가게 한다.
#[cfg(all(windows, feature = "gui"))]
pub fn complete_composition(window: &gpui_kit::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::Input::Ime::{
        CPS_COMPLETE, ImmGetContext, ImmNotifyIME, ImmReleaseContext, NI_COMPOSITIONSTR,
    };

    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(win32) = handle.as_raw()
    {
        let hwnd = win32.hwnd.get() as *mut std::ffi::c_void;
        // SAFETY: 핸들은 방금 GPUI 창에서 얻은 살아 있는 창이고, 얻은 입력 문맥은 곧바로 돌려준다.
        unsafe {
            let context = ImmGetContext(hwnd);
            if !context.is_null() {
                ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_COMPLETE, 0);
                ImmReleaseContext(hwnd, context);
            }
        }
    }
}

/// Windows가 아니면 할 일이 없다.
#[cfg(all(not(windows), feature = "gui"))]
pub fn complete_composition(_window: &gpui_kit::Window) {}

#[cfg(test)]
mod tests {
    /// 입력칸을 여는 모든 곳은 그 앞에서 터미널의 한글 조합을 확정해야 한다(`settle_composition`). 새 입력칸이 한 곳이라도 빠지면 조합이 입력칸에 끼어든다.
    #[test]
    fn test_c_ime_every_text_field_opens_after_settling_the_terminals_composition() {
        let sources = [include_str!("window_view.rs"), include_str!("accounts/window.rs")];
        let mut n_fields = 0;
        for source in sources {
            let lines: Vec<&str> = source.lines().collect();
            for (index, line) in lines.iter().enumerate().filter(|(_, line)| line.contains("InputState::new(")) {
                n_fields += 1;
                let before = &lines[index.saturating_sub(8)..index];
                assert!(before.iter().any(|line| line.contains("settle_composition")), "no settle before: {line}");
            }
        }
        assert_eq!(n_fields, 4, "rename, new file or folder, Accounts name, Accounts rename");
    }
}
