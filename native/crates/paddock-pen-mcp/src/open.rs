//! `pen_open`: 파일을 Pen에서 열고, 활성 문서가 되었는지로 성공을 판정한다.
//!
//! Pencil 도구는 `filePath`를 무시하고 활성 문서에 오류 없이 쓰는 경우가 있어서, 열기 요청이 성공했는지가 아니라 활성 문서가 대상으로 바뀌었는지를
//! 본다. 결과에는 맞는 `filePath`(저장 도구가 있는 Pen 판에서는 `uri`)와, 쓰기 앞에 붙일 문서 확인 조각(`guard`)을 함께 돌려준다.
use std::process::Command;
use std::time::Instant;

use paddock_core::pen_paths::{PenTarget, normalize_document};

use crate::limits::Limits;
use crate::pencil::{DocumentArgument, Pen};
use crate::session::Session;
#[cfg(windows)]
use crate::windows::SaveLock;

/// 결과 글: 열린 상태 설명, `execute`에 넘길 문서 인자(`filePath` 또는 `uri`), 문서 확인 조각(`guard`) 안내.
///
/// Examples
/// --------
/// | 문서 인자  | guard    | 결과 글의 구성                                                                  |
/// |------------|----------|---------------------------------------------------------------------------------|
/// | `FilePath` | `Some(g)`| 상태 줄, `filePath: <경로>`, 사용 안내, `guard: <g>`, 붙이는 법과 `WRONG_DOCUMENT` 안내 |
/// | `FilePath` | None     | 상태 줄, `filePath: <경로>`, 사용 안내, 빈 문서라 guard가 없다는 안내           |
/// | `Uri`      | `Some(g)`| 상태 줄, `uri: file:///…`, 사용 안내(`uri`를 넘기라고), 이하 같다              |
pub fn describe_opened(
    status: &str,
    document: DocumentArgument,
    windows_path: &str,
    guard: Option<&str>,
) -> String {
    let key = document.key();
    let mut lines = vec![
        status.to_string(),
        format!("{key}: {}", document.value(windows_path)),
        format!(
            "Pass the {key} above to execute as it is. An active-document spelling (`/Ubuntu/...`) falls through to another document."
        ),
    ];
    match guard {
        Some(guard) => {
            lines.push(format!("guard: {guard}"));
            lines.push("Put the guard at the very start of the input of every execute that writes. If WRONG_DOCUMENT is raised, the changes were rolled back: call pen_open again.".to_string());
        }
        None => lines.push("guard: none (empty document). Before the first write, check the active document; after you create a top-level node, call pen_open again to get a guard.".to_string()),
    }
    lines.join("\n")
}

/// 열린 결과를 설명한다. 문서 확인 조각을 얻고, 디스크에 내용이 있는데 앱이 빈 문서로 읽으면 오류로 돌려준다(그대로 쓰거나 저장하면 파일 내용을 덮어쓸 수 있다).
fn describe_loaded(
    pen: &Pen,
    status: &str,
    target: &PenTarget,
) -> Result<String, String> {
    let has_content =
        std::fs::metadata(&target.local).map_or(0, |metadata| metadata.len()) > Limits::EMPTY_DOCUMENT_MAX_BYTES;
    let deadline = Instant::now() + Limits::LOAD_DEADLINE;
    let mut guard = pen.document_guard(&target.windows);
    while guard.is_none() && has_content && Instant::now() < deadline {
        std::thread::sleep(Limits::OPEN_INTERVAL);
        guard = pen.document_guard(&target.windows);
    }
    if guard.is_none() && has_content {
        Err(format!(
            "The file has content, but the app read it as an empty document. Writing or saving could overwrite the content, so do not use Pencil on it: {}",
            target.windows
        ))
    } else {
        Ok(describe_opened(status, pen.tools().document_argument, &target.windows, guard.as_deref()))
    }
}

/// 파일이 활성 문서가 되기를 `OPEN_DEADLINE`까지 기다린다. 마지막으로 읽은 활성 문서와 함께 돌려준다.
pub fn wait_for_active(
    pen: &Pen,
    windows_path: &str,
) -> (bool, Option<String>) {
    let wanted = normalize_document(windows_path);
    let started = Instant::now();
    let deadline = started + Limits::OPEN_DEADLINE;
    let mut active: Option<String> = None;
    let mut is_active = false;
    let mut is_repeated = false;
    while !is_active && Instant::now() < deadline {
        std::thread::sleep(Limits::OPEN_INTERVAL);
        active = pen.active_document().or(active);
        is_active = active.as_deref().is_some_and(|document| normalize_document(document) == wanted);
        if !is_active && !is_repeated && started.elapsed() >= Limits::OPEN_REPEAT_AFTER {
            // 꺼져 있던 앱은 첫 요청으로 창만 열고 활성 문서를 바꾸지 않을 수 있어 한 번 더 요청한다
            is_repeated = true;
            let _ = pen.request_open(windows_path);
        }
    }
    (is_active, active)
}

/// Windows가 아닌 운영체제: 시스템 열기 명령으로 넘기기만 한다. Pencil 도구에 닿을 수 없어 활성 문서는 확인하지 못한다.
fn open_by_system(target: &PenTarget) -> Result<String, String> {
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(program)
        .arg(&target.local)
        .spawn()
        .map(|_| {
            format!(
                "Asked the system to open {}. The active document cannot be checked on this OS, so confirm in the app that it is open before using Pencil tools.",
                target.local
            )
        })
        .map_err(|error| format!("Could not open the file in Pen: {error}"))
}

/// Windows: Pen에 파일을 넘기고 활성 문서가 될 때까지 기다린다.
fn open_in_pen(target: &PenTarget) -> Result<String, String> {
    let pen = Pen::find()?;
    let before = pen.active_document();
    match before.filter(|document| normalize_document(document) == normalize_document(&target.windows)) {
        Some(document) => describe_loaded(&pen, &format!("Already the active document: {document}"), target),
        None => {
            pen.request_open(&target.windows)?;
            match wait_for_active(&pen, &target.windows) {
                (true, active) => {
                    describe_loaded(&pen, &format!("Opened. Active document: {}", active.unwrap_or_default()), target)
                }
                (false, active) => Err(format!(
                    "The active document did not change. Do not write with Pencil: a write now would go into a document that is not the target ({}).\nCheck that Pen opened the file: {}",
                    active.as_deref().unwrap_or("none"),
                    target.windows
                )),
            }
        }
    }
}

/// Windows: 저장 잠금을 쥔 채로 Pen에서 연다. 잠금 없이 활성 문서를 바꾸면, 다른 프로세스가 메뉴로 저장하는 도중일 때 그 저장이 이
/// 파일에 떨어진다.
fn open_in_pen_in_turn(target: &PenTarget) -> Result<String, String> {
    #[cfg(windows)]
    let _turn = SaveLock::acquire(Limits::SAVE_LOCK_DEADLINE).ok_or_else(|| {
        format!(
            "Another pen_open or pen_save is still running, so the file was not opened. Call again: {}",
            target.windows
        )
    })?;
    open_in_pen(target)
}

/// 파일을 연 파일 목록에 더하고 Pen에서 연다. 저장 잠금은 잡지 않는다: 잠금을 이미 쥔 `pen_save`가 대상을 활성으로 만들 때 쓴다.
///
/// 열기를 요청한 파일은 결과와 상관없이 이 프로그램이 연 파일 목록에 남는다(열기가 실패해도 창은 떠 있을 수 있다).
#[cfg(windows)]
pub fn open_without_turn(
    session: &Session,
    input: Option<&str>,
) -> Result<String, String> {
    let target = session.resolve_existing_pen_file(input)?;
    session.remember_opened(&target);
    open_in_pen(&target)
}

/// `pen_open`: 파일을 Pen에서 열고 결과 글을 돌려준다.
///
/// 이미 활성 문서면 다시 열지 않고 설명만 돌려준다. 활성 문서가 `OPEN_DEADLINE` 안에 대상으로 바뀌지 않으면 오류다.
/// 오류 문장은 에이전트에게 그대로 보인다: 없는 파일·`.pen`이 아닌 파일·Pen 미설치·활성 문서 불변·내용 있는 파일을 빈 문서로 읽음·
/// 다른 열기나 저장이 `SAVE_LOCK_DEADLINE` 넘게 이어짐(Windows).
/// 열기를 요청한 파일은 결과와 상관없이 이 프로그램이 연 파일 목록에 남는다(열기가 실패해도 창은 떠 있을 수 있다).
pub fn pen_open(
    session: &Session,
    input: Option<&str>,
) -> Result<String, String> {
    let target = session.resolve_existing_pen_file(input)?;
    session.remember_opened(&target);
    if cfg!(windows) { open_in_pen_in_turn(&target) } else { open_by_system(&target) }
}

#[cfg(windows)]
/// 이전 활성 문서로 되돌린다. 이전 활성이 없거나 지금 대상과 같거나, 파일이 사라졌거나, 이미 활성 문서면 건드리지 않는다.
///
/// 이전 문서의 창이 최소화돼 있었으면 되돌린 뒤 활성화 없이 다시 최소화한다. Pen은 문서를 활성으로 만들며 최소화된 창을 되살리므로,
/// 열기를 요청하기 전에 최소화 여부를 읽는다(대상을 활성으로 만드는 동안 이전 문서의 창은 그대로다).
pub fn restore_active(
    pen: &Pen,
    document: Option<&str>,
    windows_path: &str,
) {
    use crate::titles::{Found, file_name_of};
    use crate::windows;

    let previous = document
        .filter(|document| normalize_document(document) != normalize_document(windows_path))
        .map(paddock_core::pen_paths::document_to_windows)
        .filter(|previous| std::path::Path::new(previous).exists());
    if let Some(previous) = previous {
        let is_active =
            pen.active_document().is_some_and(|active| normalize_document(&active) == normalize_document(&previous));
        if !is_active {
            let minimized = match windows::find_editor_window(file_name_of(&previous)) {
                Found::One(window) if windows::is_minimized(window) => Some(window),
                Found::None | Found::One(_) | Found::Ambiguous => None,
            };
            if pen.request_open(&previous).is_ok() {
                wait_for_active(pen, &previous);
            }
            if let Some(window) = minimized {
                windows::minimize_without_activating(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_1_the_result_text_carries_the_file_path_and_the_guard() {
        let text = describe_opened(
            "Opened. Active document: /C:/w/a.pen",
            DocumentArgument::FilePath,
            "C:\\w\\a.pen",
            Some("const penDoc=[];"),
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "Opened. Active document: /C:/w/a.pen");
        assert_eq!(lines[1], "filePath: C:\\w\\a.pen");
        assert!(lines[2].starts_with("Pass the filePath above to execute"));
        assert_eq!(lines[3], "guard: const penDoc=[];");
        assert!(lines[4].contains("WRONG_DOCUMENT"));
    }

    #[test]
    fn test_open_2_an_empty_document_says_there_is_no_guard_yet() {
        let text = describe_opened("Opened.", DocumentArgument::FilePath, "C:\\w\\a.pen", None);
        assert!(text.contains("guard: none (empty document)"), "{text}");
        assert!(!text.contains("WRONG_DOCUMENT"));
    }

    #[test]
    fn test_open_3_a_pen_that_wants_a_uri_gets_the_file_uri_in_the_result_text() {
        let text = describe_opened("Opened.", DocumentArgument::Uri, "\\\\wsl.localhost\\Ubuntu\\h\\a.pen", None);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], "uri: file://wsl.localhost/Ubuntu/h/a.pen");
        assert!(lines[2].starts_with("Pass the uri above to execute"));
    }
}
