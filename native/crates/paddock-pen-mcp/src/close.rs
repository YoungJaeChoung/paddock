//! `pen_close`: 파일의 Pen 창을 닫아 메모리를 돌려받는다.
//!
//! 창은 파일마다 하나씩 열리고 창마다 메모리를 1 GB 넘게 쓴다(1.4 MB 파일 기준). 앱은 내용을 고치지 않아도 열 때 변경 표시를 하는
//! 경우가 있어 저장 확인만으로는 에이전트가 쓴 변경인지 가를 수 없다. 그래서 버리기는 `discard`로 명시할 때만 하고, 그 외에는 취소해 창을 남긴 채 실패로 돌려준다.
//!
//! 창 제목은 파일 이름뿐이라, 다른 폴더의 같은 이름 파일 창을 대상의 창으로 오인할 수 있다. 그래서 닫기 전에 대상이 그 창에 열려 있는지
//! 확인한다: 대상이 활성 문서면 바로 확정하고, 아니면 대상을 활성 문서로 만들어 새 창이 생기는지 본다(`identify_target`).
//!
//! ```text
//! 저장 잠금 → 이름이 같은 창 찾기 ─(없음)→ 이미 닫힘
//!   ─(하나)→ 대상이 활성인가 ─(예)→ 그 창 닫기
//!                           └(아니오)→ 대상을 활성으로 → 새 창 없음: 그 창 닫기 / 새 창이 대상: 그 새 창만 버리고 닫기(이미 닫혀 있던 것)
//!   → 이전 활성 문서로 되돌리기 → 잠금 해제
//! ```
use std::time::{Duration, Instant};

use paddock_core::pen_paths::{PenTarget, normalize_document};

use crate::limits::Limits;
use crate::open::{restore_active, wait_for_active};
use crate::pencil::Pen;
use crate::session::Session;
use crate::titles::{Found, Identified, file_name_of, identify_target};
use crate::windows::{self, CloseOutcome, SaveLock, Window};

/// 닫기를 보낸 뒤 창이 사라지거나 저장 확인이 뜨기를 기다리는 횟수와 간격(60 × 250 ms).
const CLOSE_POLLS: u32 = 60;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// `Don't Save`를 누른 뒤 창이 사라지기를 기다리는 횟수(40 × 250 ms).
const DISCARD_POLLS: u32 = 40;

/// 저장 확인의 `Don't Save`를 눌러 창이 사라지기를 `polls`번까지 기다린다.
///
/// 앞에 있지 않은 대화상자는 첫 누름을 처리하지 않는 경우가 있어, 대화상자가 남아 있는 동안 매번 다시 누른다.
/// 대화상자가 사라지면 더 누르지 않으므로 다른 창의 버튼을 누를 일은 없다.
fn discard_until_gone(
    window: Window,
    polls: u32,
) -> bool {
    let mut is_gone = false;
    for _ in 0..polls {
        if let Some(button) =
            windows::owned_dialog(window).and_then(|dialog| windows::find_button(dialog, "Don't Save"))
        {
            windows::click_button(button);
        }
        std::thread::sleep(POLL_INTERVAL);
        if !windows::is_open(window) {
            is_gone = true;
            break;
        }
    }
    is_gone
}

/// 창에 닫기를 보내고 결과를 기다린다. 저장 확인이 뜨면 `discard`일 때만 `Don't Save`를 누르고, 아니면 취소해 창을 남긴다.
fn close_window(
    window: Window,
    discard: bool,
) -> CloseOutcome {
    windows::post_close(window);
    let mut outcome = CloseOutcome::StillOpen;
    for _ in 0..CLOSE_POLLS {
        std::thread::sleep(POLL_INTERVAL);
        if !windows::is_open(window) {
            outcome = CloseOutcome::Closed;
            break;
        }
        if let Some(dialog) = windows::owned_dialog(window) {
            outcome = if !discard {
                windows::cancel_dialog(dialog);
                CloseOutcome::Unsaved
            } else if windows::find_button(dialog, "Don't Save").is_some() {
                if discard_until_gone(window, DISCARD_POLLS) { CloseOutcome::Closed } else { CloseOutcome::StillOpen }
            } else {
                CloseOutcome::UnknownDialog
            };
            break;
        }
    }
    outcome
}

/// 버리고 닫기가 실패하면 한 번 더 닫기부터 다시 한다.
///
/// 저장 확인을 한 번 취소한 창은 다음 닫기의 `Don't Save`를 처리하지 않는 경우가 있고, 같은 절차를 다시 하면 닫힌다.
/// 버리지 않는 닫기는 다시 하지 않는다(사람이 저장을 고르게 둔다).
fn close_with_second_try(
    window: Window,
    discard: bool,
) -> CloseOutcome {
    let first = close_window(window, discard);
    if discard && matches!(first, CloseOutcome::StillOpen) && windows::is_open(window) {
        close_window(window, discard)
    } else {
        first
    }
}

/// 이름이 같은 창 하나(`named`)가 대상의 창인지 확인한다. 저장 잠금을 쥔 채로 불린다.
///
/// 대상이 이미 활성 문서면 그 창이다. 아니면 대상을 활성 문서로 만들고 새 문서 창이 생겼는지 `WINDOW_DEADLINE`까지 본다(새 창의 제목은
/// 잠깐 뒤에 파일 이름이 된다). 대상을 활성으로 만들지 못하면 에이전트에게 보일 문장을 오류로 돌려준다.
fn identify(
    pen: &Pen,
    target: &PenTarget,
    named: Window,
) -> Result<Identified<Window>, String> {
    let is_active =
        pen.active_document().is_some_and(|active| normalize_document(&active) == normalize_document(&target.windows));
    if is_active {
        Ok(Identified::Open(named))
    } else {
        let before: Vec<Window> = windows::document_windows().into_iter().map(|(window, _)| window).collect();
        match pen.request_open(&target.windows).map(|()| wait_for_active(pen, &target.windows).0) {
            Ok(true) => {
                let file_name = file_name_of(&target.windows);
                let deadline = Instant::now() + Limits::WINDOW_DEADLINE;
                let mut identified = identify_target(named, &before, &windows::document_windows(), file_name);
                while identified == Identified::Unknown && Instant::now() < deadline {
                    std::thread::sleep(Limits::WINDOW_INTERVAL);
                    identified = identify_target(named, &before, &windows::document_windows(), file_name);
                }
                Ok(identified)
            }
            Ok(false) => Err(format!(
                "Could not make the file the active document, so it is not known which Pen window is its; none was closed: {}",
                target.windows
            )),
            Err(error) => Err(error),
        }
    }
}

/// 대상의 창을 확인하고 닫는다. 저장 잠금을 쥔 채로 불리고, 대상을 활성으로 만들었으면 이전 활성 문서로 되돌린다.
fn close_target(
    pen: &Pen,
    target: &PenTarget,
    discard: bool,
) -> Result<String, String> {
    let file_name = file_name_of(&target.windows).to_string();
    let previous = pen.active_document();
    let result = match windows::find_document(&file_name) {
        Found::None => Ok(format!("Already closed: {}", target.windows)),
        Found::Ambiguous => Err(format!("There are several Pen windows named {file_name}, so none was closed.")),
        Found::One(named) => identify(pen, target, named).and_then(|identified| match identified {
            Identified::Open(window) => match close_with_second_try(window, discard) {
                CloseOutcome::Closed => Ok(format!("Closed: {}", target.windows)),
                CloseOutcome::Unsaved => Err(format!(
                    "A save prompt appeared, so the close was cancelled and the window is still open. If you wrote content, call pen_save and then this again; if you wrote nothing, call it with discard: true: {}",
                    target.windows
                )),
                CloseOutcome::UnknownDialog => Err(format!(
                    "Could not close the window: a dialog without a Don't Save button appeared: {}",
                    target.windows
                )),
                CloseOutcome::StillOpen => {
                    Err(format!("Could not close the window; it is still open: {}", target.windows))
                }
            },
            // 확인하려고 방금 연 창이라 쓴 내용이 없다. 앱이 열 때 붙이는 변경 표시는 버린다.
            Identified::OpenedForCheck(opened) => match close_with_second_try(opened, true) {
                CloseOutcome::Closed => Ok(format!(
                    "Already closed: {}. The open Pen window named {file_name} belongs to another file and was left open.",
                    target.windows
                )),
                CloseOutcome::Unsaved | CloseOutcome::UnknownDialog | CloseOutcome::StillOpen => Err(format!(
                    "The file was not open, and the window opened to check it could not be closed. The open window named {file_name} belongs to another file and was left open: {}",
                    target.windows
                )),
            },
            Identified::Unknown => Err(format!(
                "Could not tell whether the Pen window named {file_name} is this file's, so none was closed: {}",
                target.windows
            )),
        }),
    };
    restore_active(pen, previous.as_deref(), &target.windows);
    result
}

/// `pen_close`: 파일의 Pen 창을 닫고 결과 글을 돌려준다.
///
/// 창이 이미 없으면 닫혀 있다고 알린다. 이름이 같은 창이 대상의 창인지 확인하고(그 사이 대상을 잠시 활성 문서로 만들 수 있다), 확인되지
/// 않으면 닫지 않는다. 이름이 같은 창이 여럿이거나 저장 확인에서 취소했거나 창이 남으면 오류다.
/// 대상을 활성 문서로 만드는 동안 다른 열기·저장이 끼어들지 않도록 저장 잠금을 쥐고, 다른 열기·저장이 `SAVE_LOCK_DEADLINE` 넘게
/// 이어지면 아무것도 하지 않고 실패한다.
/// 닫혔으면(이미 닫혀 있던 경우 포함) 이 프로그램이 연 파일 목록에서 빼, 저장 뒤 수정 시각 비교 대상에서 제외한다.
pub fn pen_close(
    session: &Session,
    input: Option<&str>,
    discard: bool,
) -> Result<String, String> {
    let target = session.resolve(input)?;
    let pen = Pen::find()?;
    let result = match SaveLock::acquire(Limits::SAVE_LOCK_DEADLINE) {
        None => Err(format!(
            "Another pen_open or pen_save is still running, so no window was closed. Call again: {}",
            target.windows
        )),
        Some(_turn) => close_target(&pen, &target, discard),
    };
    if result.is_ok() {
        session.forget_opened(&target);
    }
    result
}
