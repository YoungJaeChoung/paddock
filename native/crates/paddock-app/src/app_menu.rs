//! macOS 상단 메뉴 막대와 앱 종료 키(Cmd+Q). 앱 이름 메뉴의 Quit, Hide·Hide Others·Show All, Edit의 Cut·Copy·Paste·Select All, View의 Toggle Sidebar, Window의 Minimize가 있다. 항목의 키(Cmd+Q·Cmd+B·Cmd+M·Cmd+H·Cmd+Option+H)는 키 연결과 묶여 메뉴에 표시된다.
//!
//! macOS 앱은 메뉴 막대가 있어야 Cmd+Q·Cmd+C 같은 표준 키가 시스템 규칙대로 동작한다. 이 메뉴는 표준 항목의 최소 묶음이다.
//! Edit의 항목은 입력칸·편집기가 받는 동작이라 그것들에 포커스가 있을 때만 켜진다. 터미널의 복사·붙여넣기는 키(Cmd+C·Cmd+V)로 한다.
use gpui_kit::component::input::{Copy, Cut, Paste, SelectAll};
use gpui_kit::{
    AnyWindowHandle, App, Global, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, WeakEntity, actions,
};

use crate::window_view::WorkWindow;

actions!(paddock_app, [Quit, ToggleSidebar, MinimizeWindow, HideApp, HideOtherApps, ShowAllApps]);

/// 메뉴 동작이 향할 본 창. 본 창을 열 때 한 번 정해진다.
struct MainWindow {
    window: AnyWindowHandle,
    view: WeakEntity<WorkWindow>,
}

impl Global for MainWindow {}

/// 본 창을 메뉴 동작의 대상으로 등록한다.
pub fn register_main_window(
    window: AnyWindowHandle,
    view: WeakEntity<WorkWindow>,
    cx: &mut App,
) {
    cx.set_global(MainWindow { window, view });
}

/// 메뉴 막대와 Cmd+Q를 등록한다. 앱 시작 때 한 번 부른다.
pub fn install(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| {
        cx.defer(|cx| run_on_main(cx, |view, window, cx| view.request_quit(window, cx)));
    });
    cx.on_action(|_: &ToggleSidebar, cx| {
        cx.defer(|cx| run_on_main(cx, |view, window, cx| view.toggle_sidebar(window, cx)));
    });
    cx.on_action(|_: &MinimizeWindow, cx| {
        if let Some(window) = cx.active_window() {
            window.update(cx, |_, window, _| window.minimize_window()).ok();
        }
    });
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    // 메뉴 항목에 키가 표시되도록 키 연결과 묶는다. Cmd+B는 앱 단축키 처리(`app_shortcut`)와 같은 동작이다.
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
    ]);
    cx.set_menus(vec![
        Menu::new("Paddock Native").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Paddock Native", HideApp),
            MenuItem::action("Hide Others", HideOtherApps),
            MenuItem::action("Show All", ShowAllApps),
            MenuItem::separator(),
            MenuItem::action("Quit Paddock Native", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([MenuItem::action("Toggle Sidebar", ToggleSidebar)]),
        Menu::new("Window").items([MenuItem::action("Minimize", MinimizeWindow)]),
    ]);
}

/// 본 창의 `WorkWindow`에서 `run`을 실행한다. 본 창이 이미 닫혔으면 아무 일도 하지 않는다.
/// 동작 처리기는 창이 키·메뉴 사건을 처리하는 도중에 불리므로, 호출자는 `cx.defer`로 그 처리가 끝난 뒤에 이 함수를 불러야 창을 다시 빌릴 수 있다.
fn run_on_main(
    cx: &mut App,
    run: impl FnOnce(&mut WorkWindow, &mut gpui_kit::Window, &mut gpui_kit::Context<WorkWindow>),
) {
    let target = cx.try_global::<MainWindow>().and_then(|main| main.view.upgrade().map(|view| (main.window, view)));
    if let Some((window, view)) = target {
        window.update(cx, |_, window, cx| view.update(cx, |this, cx| run(this, window, cx))).ok();
    }
}
