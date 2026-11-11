//! `.pen` 파일 열기: 탭을 만들지 않고 운영체제 파일 연결로 Pen 데스크톱 앱에 넘긴다.
//!
//! `.pen`은 Pen 앱이 그리는 문서라 이 앱의 텍스트 탭으로 열면 암호화된 내용이 보일 뿐이다. 변경 비교 탭을 뺀 모든 파일 입구가
//! `open_file`을 지나므로 그곳에서 이 흐름으로 보낸다. 열기 실패는 상태 줄 한 줄로 알린다.
use gpui_kit::{Context, Window};

use crate::window_view::WorkWindow;

/// Windows에서 `.pen`을 Pen에 넘긴다. 파일 연결이 있으면 셸에 맡기고, 연결이 없으면 설치 위치의 `Pen.exe`를 직접 띄운다.
/// 느릴 수 있어 창 스레드가 아닌 곳에서 부른다.
///
/// `Pen.exe`는 표준 입출력 셋을 모두 닫아 띄운다. 이 앱의 출력 통로를 물려받으면 Pen이 그 통로가 끊겼을 때 `write EPIPE` 오류 창을 띄운다.
/// 연결도 설치 위치의 `Pen.exe`도 없으면 `NotInstalled`이다.
#[cfg(windows)]
fn open_in_pen(path: &str) -> Result<(), paddock_core::pen_paths::PenOpenFailure> {
    use std::process::{Command, Stdio};

    use paddock_core::pen_paths::{PenHost, PenOpenFailure, pen_install_candidates, resolve_target};

    use super::open::{NO_APP_REASON, shell_open};

    let target = resolve_target(path, PenHost::Windows, None)
        .map_err(|error| PenOpenFailure::LaunchFailed(error.to_string()))?;
    match shell_open(&target.windows) {
        Err(reason) if reason == NO_APP_REASON => {
            let program_files = std::env::var("ProgramFiles").unwrap_or_default();
            let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
            let installed = pen_install_candidates(&program_files, &local_app_data)
                .into_iter()
                .find(|exe| std::path::Path::new(exe).is_file());
            match installed {
                Some(exe) => Command::new(exe)
                    .arg(&target.windows)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map(|_| ())
                    .map_err(|error| PenOpenFailure::LaunchFailed(error.to_string())),
                None => Err(PenOpenFailure::NotInstalled),
            }
        }
        Err(reason) => Err(PenOpenFailure::LaunchFailed(reason)),
        Ok(()) => Ok(()),
    }
}

impl WorkWindow {
    /// `.pen` 파일을 탭 없이 Pen 앱에 넘긴다. Windows는 작업 스레드에서 열고 결과를 상태 줄에 쓰며,
    /// macOS·Linux는 기본 앱 열기 명령을 띄운다(띄우지 못하면 상태 줄에 알린다).
    pub(crate) fn open_pen(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(windows)]
        {
            let path = path.to_string();
            cx.spawn_in(window, async move |this, cx| {
                let outcome = cx.background_executor().spawn(async move { open_in_pen(&path) }).await;
                if let Err(reason) = outcome {
                    let _ = this.update_in(cx, |this, _window, cx| {
                        this.status = Some(paddock_core::pen_paths::pen_open_failure(&reason));
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        #[cfg(not(windows))]
        {
            use super::open::open_command;

            let _ = (&window, &cx);
            let (program, args) = open_command(std::env::consts::OS, path, false);
            self.spawn_external(&program, &args, "open the file in Pen");
        }
    }
}
