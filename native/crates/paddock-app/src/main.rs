//! Paddock 네이티브 앱 실행 진입점: 창을 먼저 열고, 데몬 연결과 터미널 되살리기는 창 안에서 한다.
#![cfg_attr(windows, windows_subsystem = "windows")]

use gpui_kit::assets::Assets;
use gpui_kit::{
    App, AppContext, TitlebarOptions, WindowBounds, WindowOptions, application, init, open_window, px, size,
};
use paddock_app::window_view::WorkWindow;
use paddock_app::{app_log, fatal_report};

/// 처음 여는 창의 크기(논리 px).
mod window_size {
    pub const WIDTH: f32 = 1000.0;
    pub const HEIGHT: f32 = 640.0;
    /// `--offscreen` 창이 대본의 `resize`로 커져도 화면에 걸치지 않도록 왼쪽으로 비켜 두는 폭.
    #[cfg(feature = "automation")]
    pub const OFFSCREEN_RESERVED_WIDTH: f32 = 4000.0;
    /// `--offscreen`일 때 창의 위쪽 y 좌표.
    #[cfg(feature = "automation")]
    pub const OFFSCREEN_Y: f32 = 100.0;
}

const WINDOW_TITLE: &str = "Paddock";

fn main() {
    // 자동 점검의 화면 밖 실행은 사용자 화면에 오류 창을 띄우지 않는다.
    let quiet = cfg!(feature = "automation") && std::env::args().any(|arg| arg == "--offscreen");
    fatal_report::install(!quiet);
    app_log::event("app started");
    application().with_assets(Assets).run(move |cx| {
        init(cx);
        paddock_app::window_view::bind_keys(cx);
        paddock_app::chrome::sync_component_theme(cx);
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        open_main_window(cx);
    });
}

fn window_options(cx: &App) -> WindowOptions {
    let bounds = WindowBounds::centered(size(px(window_size::WIDTH), px(window_size::HEIGHT)), cx);
    // Windows는 제목줄을 앱이 직접 그린다(`titlebar`). 다른 OS는 시스템 제목줄을 그대로 쓴다.
    WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions {
            title: Some(WINDOW_TITLE.into()),
            appears_transparent: cfg!(windows),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// 앱 창을 연다. 창을 닫으면 앱이 끝나지만 데몬과 터미널은 남는다(종료 묻기에서 끝내기를 고르지 않는 한).
fn open_main_window(cx: &mut App) {
    #[cfg(feature = "automation")]
    let args = AutomationArgs::from_command_line();
    #[cfg_attr(not(feature = "automation"), allow(unused_mut))]
    let mut options = window_options(cx);
    // 자동 점검은 사용자의 화면과 포커스를 가리지 않도록 화면 밖에 활성화 없이 띄울 수 있다.
    #[cfg(feature = "automation")]
    if args.offscreen {
        options.focus = false;
        options.window_bounds = Some(WindowBounds::Windowed(gpui_kit::Bounds::new(
            gpui_kit::point(
                px(paddock_app::automation::offscreen_x(window_size::OFFSCREEN_RESERVED_WIDTH)),
                px(window_size::OFFSCREEN_Y),
            ),
            size(px(window_size::WIDTH), px(window_size::HEIGHT)),
        )));
    }
    #[cfg(feature = "automation")]
    let hooks = args.script.and_then(|path| {
        let daemon_exe = paddock_app::daemon_launch::sibling_daemon_exe().ok()?;
        let hooks = paddock_app::automation::Hooks::new(daemon_exe);
        paddock_app::automation::run(path, hooks.clone());
        Some(hooks)
    });
    open_window(options, cx, move |window, cx| {
        let view = cx.new(|cx| WorkWindow::new(window, cx));
        #[cfg(feature = "automation")]
        if let Some(hooks) = hooks {
            view.update(cx, |view, cx| view.start_automation(hooks, window, cx));
        }
        view
    })
    .expect("Failed to open window");
}

/// 자동 점검 대본을 켜는 실행 인자. `automation` 기능으로 빌드한 개발용 실행 파일만 읽는다.
#[cfg(feature = "automation")]
struct AutomationArgs {
    script: Option<std::path::PathBuf>,
    /// 창을 화면 밖에 활성화 없이 띄운다(`--offscreen`).
    offscreen: bool,
}

#[cfg(feature = "automation")]
impl AutomationArgs {
    /// `--script 파일`과 `--out 폴더`를 읽는다. 결과 폴더가 없으면 임시 폴더 아래에 만든다.
    fn from_command_line() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |key: &str| args.iter().position(|arg| arg == key).and_then(|i| args.get(i + 1)).cloned();
        let out = value("--out")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("paddock-app-out"));
        paddock_app::automation::init(out);
        Self {
            script: value("--script").map(std::path::PathBuf::from),
            offscreen: args.iter().any(|arg| arg == "--offscreen"),
        }
    }
}
