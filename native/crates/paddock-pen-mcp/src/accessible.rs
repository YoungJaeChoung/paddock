//! 접근성 기능(MSAA, `IAccessible`)으로 창 안의 단추·메뉴 항목을 읽고 기본 동작을 부른다. 키·마우스 입력 없이 Pen 메뉴를 다루는 수단이다.
//!
//! Pen 메뉴에는 MSAA만 듣는다: UI Automation의 펼치기는 실패하며 앞 창을 빼앗고, UI Automation을 거친 MSAA 기본 동작은 아무 일도 하지 않았다.
use ::windows::Win32::Foundation::HWND;
use ::windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IDispatch};
use ::windows::Win32::System::Variant::VARIANT;
use ::windows::Win32::UI::Accessibility::{
    AccessibleChildren, AccessibleObjectFromWindow, IAccessible, ROLE_SYSTEM_DOCUMENT,
};
use ::windows::core::Interface;
use windows_sys::Win32::UI::WindowsAndMessaging::OBJID_CLIENT;

use crate::windows::Window;

/// 접근성 트리를 훑는 범위의 상한. 웹 문서 안으로는 내려가지 않으므로 창의 틀(메뉴 막대·메뉴 팝업)만 읽는다.
struct Walk;

impl Walk {
    /// 창 루트에서 내려가는 깊이.
    const MAX_DEPTH: usize = 12;
    /// 한 요소에서 읽는 자식 수.
    const MAX_CHILDREN: i32 = 400;
    /// 한 창에서 모으는 요소 수.
    const MAX_ELEMENTS: usize = 4000;
}

/// 요소 자신을 가리키는 자식 번호(`CHILDID_SELF`).
const CHILD_SELF: i32 = 0;

/// 이 스레드의 COM 사용 구간. 만들 때 단일 스레드 아파트로 들어가고, 버릴 때 나온다. 이미 다른 방식으로 들어가 있으면 그대로 쓴다.
///
/// 접근성 객체는 이 값보다 먼저 버려져야 한다(같은 함수 안에서 이 값을 먼저 만들면 Rust가 나중에 버린다).
pub struct ComApartment {
    is_entered: bool,
}

impl ComApartment {
    pub fn enter() -> Self {
        // SAFETY: 예약 인자는 비운다. 성공했을 때만 `Drop`에서 짝을 맞춰 나온다.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self { is_entered: result.is_ok() }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.is_entered {
            // SAFETY: 같은 스레드에서 성공한 `CoInitializeEx`와 짝을 맞춘다.
            unsafe { CoUninitialize() };
        }
    }
}

/// 접근성 트리의 요소 하나. 자식 번호로만 불리는 단순 요소는 부모 객체와 그 번호로 가리킨다.
pub struct Element {
    host: IAccessible,
    child: VARIANT,
    pub name: String,
    pub role: u32,
}

impl Element {
    fn read(
        host: IAccessible,
        child: VARIANT,
    ) -> Self {
        // SAFETY: `host`는 살아 있는 접근성 객체이고 `child`는 그 객체가 준 자식 번호다. 실패하면 빈 이름·역할 0으로 둔다.
        let name = unsafe { host.get_accName(&child) }.map(|name| name.to_string()).unwrap_or_default();
        // SAFETY: 위와 같다.
        let role = unsafe { host.get_accRole(&child) }
            .ok()
            .and_then(|role| i32::try_from(&role).ok())
            .and_then(|role| u32::try_from(role).ok())
            .unwrap_or(0);
        Self { host, child, name, role }
    }

    /// 요소의 기본 동작(단추 누르기·메뉴 항목 고르기)을 부른다. 실패하면 운영체제 오류 문장을 돌려준다.
    pub fn do_default_action(&self) -> Result<(), String> {
        // SAFETY: 읽을 때와 같은 객체와 자식 번호다.
        unsafe { self.host.accDoDefaultAction(&self.child) }.map_err(|error| error.to_string())
    }
}

/// 창의 클라이언트 영역 접근성 객체.
fn root_of(window: Window) -> Result<IAccessible, String> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: 인터페이스 번호는 `IAccessible`의 것이고, 성공하면 `raw`에 참조 하나가 담긴다.
    unsafe { AccessibleObjectFromWindow(HWND(window.handle()), OBJID_CLIENT as u32, &IAccessible::IID, &mut raw) }
        .map_err(|error| error.to_string())?;
    if raw.is_null() {
        Err("The window gave no accessibility object.".to_string())
    } else {
        // SAFETY: 위 호출이 `IAccessible` 참조 하나를 넘겼고, 그 소유를 여기서 받는다.
        Ok(unsafe { IAccessible::from_raw(raw) })
    }
}

/// `parent`의 자식들을 `found`에 더하고, 객체인 자식으로 내려간다. 웹 문서(역할 `ROLE_SYSTEM_DOCUMENT`) 안으로는 내려가지 않는다.
fn collect_children(
    parent: &IAccessible,
    depth: usize,
    found: &mut Vec<Element>,
) {
    // SAFETY: `parent`는 살아 있는 접근성 객체다.
    let n_children = unsafe { parent.accChildCount() }.unwrap_or(0).clamp(0, Walk::MAX_CHILDREN);
    if depth < Walk::MAX_DEPTH && n_children > 0 && found.len() < Walk::MAX_ELEMENTS {
        let mut children = vec![VARIANT::default(); n_children as usize];
        let mut n_obtained = 0i32;
        // SAFETY: `children`의 길이만큼 자식을 받아 오고, 받은 개수만큼만 읽는다.
        if unsafe { AccessibleChildren(parent, 0, &mut children, &mut n_obtained) }.is_ok() {
            for child in children.into_iter().take(usize::try_from(n_obtained).unwrap_or(0)) {
                // 자식이 객체면 그 객체 자신이 요소이고, 번호면 부모가 그 번호의 요소를 대신 답한다.
                let nested = IDispatch::try_from(&child).ok().and_then(|dispatch| dispatch.cast::<IAccessible>().ok());
                let is_object = nested.is_some();
                let element = match nested {
                    Some(object) => Element::read(object, VARIANT::from(CHILD_SELF)),
                    None => Element::read(parent.clone(), child),
                };
                let next = (is_object && element.role != ROLE_SYSTEM_DOCUMENT).then(|| element.host.clone());
                found.push(element);
                if let Some(next) = next {
                    collect_children(&next, depth + 1, found);
                }
            }
        }
    }
}

/// 창의 접근성 요소 목록(웹 문서 안은 빼고). 창의 접근성 객체를 얻지 못하면 그 사유를 오류로 돌려준다.
///
/// 호출하는 스레드는 COM에 들어가 있어야 한다(`ComApartment`).
pub fn elements(window: Window) -> Result<Vec<Element>, String> {
    let root = root_of(window)?;
    let mut found = Vec::new();
    collect_children(&root, 0, &mut found);
    Ok(found)
}
