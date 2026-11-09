//! Windows 창·프로세스 조작: Pen 창과 그 메뉴 팝업 창을 찾고, 최소화를 활성화 없이 풀거나 되돌리고, 닫기를 보낸다.
//!
//! Pen은 파일마다 창을 하나씩 열고 프로세스 정보로는 창 하나만 알 수 있어, 모든 보이는 창을 훑어 제목으로 찾는다.
//! 키 입력은 쓰지 않는다: 새 문서 창은 모니터 밖에 열리기도 하고, 그 창에서는 단축키가 듣지 않는다.
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::limits::Limits;
use crate::titles::{Found, find_by_title};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, HWND, INVALID_HANDLE_VALUE, LPARAM, SetHandleInformation, WAIT_ABANDONED,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_CLICK, EnumChildWindows, EnumWindows, GW_OWNER, GetClassNameW, GetWindow, GetWindowTextW,
    GetWindowThreadProcessId, IDCANCEL, IsIconic, IsWindow, IsWindowVisible, PostMessageW, SW_SHOWMINNOACTIVE,
    SW_SHOWNOACTIVATE, ShowWindow, WM_CLOSE, WM_COMMAND,
};

/// Pen이 만드는 창의 클래스 이름.
struct WindowClass;

impl WindowClass {
    /// 문서 창과 메뉴 팝업 창(Chromium 창).
    const CHROMIUM: &'static str = "Chrome_WidgetWin_1";
    /// 저장 확인 같은 대화상자.
    const DIALOG: &'static str = "#32770";
}

/// 운영체제 창 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window(isize);

impl Window {
    fn from_handle(handle: HWND) -> Self {
        Self(handle as isize)
    }

    /// 운영체제 창 핸들. 접근성 기능(MSAA)에 창을 넘길 때 쓴다.
    pub fn handle(self) -> HWND {
        self.0 as HWND
    }
}

/// 닫기 요청의 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    Closed,
    /// 저장 확인이 떠서 취소했다. 창은 그대로다.
    Unsaved,
    /// 저장 확인에 `Don't Save` 버튼이 없다.
    UnknownDialog,
    StillOpen,
}

/// 저장하는 동안 잡는 이름 있는 뮤텍스. 이 프로그램이 여럿 떠 있어도(Claude 세션마다 하나) 같은 순간에 저장을 다루는 쪽은 하나여야 한다:
/// 메뉴의 Save는 그 메뉴가 달린 창이 아니라 앱의 활성 문서를 저장하므로, 두 프로세스가 활성 문서를 번갈아 바꾸며 메뉴를 누르면 다른 파일이
/// 저장된다. 놓지 않고 죽어도 운영체제가 다음 대기자에게 넘긴다.
///
/// 잡는 쪽은 이 프로그램에서 활성 문서를 바꾸는 두 도구다. `pen_save`는 대상을 활성으로 만들기부터 이전 활성 문서로 되돌리기까지 쥐고,
/// `pen_open`은 여는 동안 쥔다. 그래서 이 프로그램끼리는 저장 도중에 활성 문서를 바꾸지 못한다. 사람이 앱에서 문서를 바꾸거나
/// `pen_close`가 활성 문서의 창을 닫는 일은 막지 못한다. 그 틈은 Save를 고르기 직전의 활성 문서 재확인으로 좁힐 뿐이다.
///
/// 잠금은 머신 전체에 하나다(`Global\` 이름 공간). 이름은 같은 방식으로 Pen 메뉴를 눌러 저장하는 pen-open MCP 서버의 잠금과 같아서,
/// 두 프로그램이 함께 등록돼 있어도 두 쪽의 저장은 서로를 기다리고, 이 프로그램의 열기도 그쪽 저장을 기다린다(그쪽 열기는 잠금을
/// 잡지 않는다). 이름을 바꾸면 그 배제가 사라진다.
///
/// 같은 스레드가 이미 쥔 잠금을 다시 잡으면 바로 잡힌다(Windows 뮤텍스는 소유 스레드의 거듭 잡기를 센다).
pub struct SaveLock(HANDLE);

impl SaveLock {
    const NAME: &'static str = "Global\\PenOpenSave";

    /// 잠금을 `deadline`까지 기다려 잡는다. 시한을 넘기면 None이다(호출자는 저장을 부르지 않는다).
    pub fn acquire(deadline: Duration) -> Option<Self> {
        let name: Vec<u16> = Self::NAME.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: 이름은 NUL로 끝나고 호출 동안 살아 있다. 보안 속성은 기본값(null)이다.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            None
        } else {
            let millis = u32::try_from(deadline.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: 방금 만든 뮤텍스 핸들이다.
            let waited = unsafe { WaitForSingleObject(handle, millis) };
            if waited == WAIT_OBJECT_0 || waited == WAIT_ABANDONED {
                Some(Self(handle))
            } else {
                // SAFETY: 이 핸들은 여기서만 쥐고 있다.
                unsafe { CloseHandle(handle) };
                None
            }
        }
    }
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        // SAFETY: `acquire`가 잡은 같은 스레드에서 놓고 닫는다.
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

/// 이 프로세스의 표준 입출력 핸들을 자식 프로세스가 물려받지 못하게 한다.
///
/// 자식을 띄울 때 핸들 상속이 켜지므로, 이 프로세스가 Claude에게서 받은 입출력 통로가 Pen에 상속되어 통로가 끊길 때 Pen이 `write EPIPE`
/// 오류 창을 띄우는 일을 막는다. 이 프로세스 자신의 입출력에는 영향이 없다.
pub fn disable_stdio_inheritance() {
    use std::os::windows::io::AsRawHandle;

    let handles: [HANDLE; 3] =
        [std::io::stdin().as_raw_handle(), std::io::stdout().as_raw_handle(), std::io::stderr().as_raw_handle()];
    for handle in handles {
        // SAFETY: 이 프로세스가 가진 표준 입출력 핸들의 상속 표지만 끈다. 닫거나 바꾸지 않는다. 유효하지 않은 핸들이면 호출이 실패하고 끝난다.
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
        }
    }
}

/// `Pen.exe` 프로세스의 번호 집합.
fn pen_processes() -> HashSet<u32> {
    let mut found = HashSet::new();
    // SAFETY: 스냅숏 핸들은 이 함수 안에서만 쓰고 닫는다. `PROCESSENTRY32W`는 0으로 채운 뒤 크기를 적어 넘긴다(API 규약).
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let length = entry.szExeFile.iter().position(|&unit| unit == 0).unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case("Pen.exe") {
                    found.insert(entry.th32ProcessID);
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
    }
    found
}

/// 창을 만든 프로세스의 번호.
fn process_of(window: Window) -> u32 {
    let mut process = 0u32;
    // SAFETY: 없는 핸들이면 0을 남기고 끝난다. `process`는 쓸 수 있는 자리다.
    unsafe { GetWindowThreadProcessId(window.handle(), &mut process) };
    process
}

/// `EnumWindows` 콜백으로 넘기는 상태.
struct Collect<'a> {
    processes: &'a HashSet<u32>,
    found: Vec<Window>,
}

unsafe extern "system" fn collect_visible(
    window: HWND,
    data: LPARAM,
) -> i32 {
    // SAFETY: `data`는 `pen_windows`가 넘긴 `Collect`의 주소이고, 열거가 끝날 때까지 살아 있다.
    let collect = unsafe { &mut *(data as *mut Collect) };
    let window = Window::from_handle(window);
    // SAFETY: `window`는 열거가 준 핸들이다.
    if collect.processes.contains(&process_of(window)) && unsafe { IsWindowVisible(window.handle()) } != 0 {
        collect.found.push(window);
    }
    1
}

/// Pen 프로세스가 가진 보이는 최상위 창. 최소화된 창도 보이는 창이다.
fn pen_windows() -> Vec<Window> {
    let processes = pen_processes();
    let mut collect = Collect { processes: &processes, found: Vec::new() };
    // SAFETY: 콜백은 `collect`만 읽고 쓰며, `EnumWindows`는 반환 전에 열거를 끝낸다.
    unsafe { EnumWindows(Some(collect_visible), &mut collect as *mut Collect as LPARAM) };
    collect.found
}

fn text_of(
    window: Window,
    read: unsafe extern "system" fn(HWND, *mut u16, i32) -> i32,
) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: 버퍼 길이를 그대로 넘기고, 돌려받은 글자 수만큼만 읽는다.
    let length = unsafe { read(window.handle(), buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..length.clamp(0, buffer.len() as i32) as usize])
}

fn title(window: Window) -> String {
    text_of(window, GetWindowTextW)
}

fn class_name(window: Window) -> String {
    text_of(window, GetClassNameW)
}

fn owner_of(window: Window) -> HWND {
    // SAFETY: 핸들 조회다. 소유 창이 없으면 null이다.
    unsafe { GetWindow(window.handle(), GW_OWNER) }
}

/// Pen 문서 창 전부와 그 제목. 대화상자(`#32770`)와 다른 창이 소유한 창(메뉴 팝업)은 뺀다.
///
/// 막 열리는 문서 창은 제목이 파일 이름이 되기 전(`Pen` 등)에도 들어 있다.
pub fn document_windows() -> Vec<(Window, String)> {
    pen_windows()
        .into_iter()
        .filter(|window| class_name(*window) != WindowClass::DIALOG && owner_of(*window).is_null())
        .map(|window| (window, title(window)))
        .collect()
}

/// 제목이 파일 이름과 같은(대소문자 무시) Pen 문서 창.
pub fn find_document(file_name: &str) -> Found<Window> {
    find_by_title(&document_windows(), file_name)
}

/// 제목이 파일 이름과 같은(대소문자 무시) Pen 편집 창. 메뉴를 다룰 수 있는 Chromium 창(`Chrome_WidgetWin_1`)만 본다.
/// 창이 나타나기를 기다리지 않는다(기다리려면 `wait_for_editor_window`).
pub fn find_editor_window(file_name: &str) -> Found<Window> {
    let titled: Vec<(Window, String)> = pen_windows()
        .into_iter()
        .filter(|window| class_name(*window) == WindowClass::CHROMIUM)
        .map(|window| (window, title(window)))
        .collect();
    find_by_title(&titled, file_name)
}

/// 편집 창이 나타나기를 `WINDOW_INTERVAL` 간격으로 `WINDOW_DEADLINE`까지 기다려 찾는다.
pub fn wait_for_editor_window(file_name: &str) -> Found<Window> {
    let deadline = Instant::now() + Limits::WINDOW_DEADLINE;
    let mut found = find_editor_window(file_name);
    while found == Found::None && Instant::now() < deadline {
        std::thread::sleep(Limits::WINDOW_INTERVAL);
        found = find_editor_window(file_name);
    }
    found
}

pub fn is_open(window: Window) -> bool {
    // SAFETY: 없는 핸들을 줘도 거짓을 돌려준다.
    unsafe { IsWindow(window.handle()) != 0 }
}

pub fn is_minimized(window: Window) -> bool {
    // SAFETY: 없는 핸들을 줘도 거짓을 돌려준다.
    unsafe { IsIconic(window.handle()) != 0 }
}

/// 최소화된 창을 원래 자리로 되살리되 활성 창으로 만들지 않는다. 앞 창(포커스)은 그대로다.
pub fn show_without_activating(window: Window) {
    // SAFETY: 창 핸들만 받는 요청이다.
    unsafe { ShowWindow(window.handle(), SW_SHOWNOACTIVATE) };
}

/// 창을 다시 최소화하되 다른 창을 활성으로 바꾸지 않는다.
pub fn minimize_without_activating(window: Window) {
    // SAFETY: 창 핸들만 받는 요청이다.
    unsafe { ShowWindow(window.handle(), SW_SHOWMINNOACTIVE) };
}

/// 문서 창이 띄운 팝업 창들. 같은 프로세스의 보이는 Chromium 창 가운데 제목이 비었고 소유 창이 그 문서 창인 것이다.
///
/// File 메뉴만이 아니라 다른 메뉴·드롭다운 팝업도 같은 조건이라, 어느 것이 File 메뉴인지는 호출자가 내용으로 가린다.
pub fn menu_popups(window: Window) -> Vec<Window> {
    let process = process_of(window);
    pen_windows()
        .into_iter()
        .filter(|candidate| {
            process_of(*candidate) == process
                && owner_of(*candidate) == window.handle()
                && class_name(*candidate) == WindowClass::CHROMIUM
                && title(*candidate).is_empty()
        })
        .collect()
}

pub fn post_close(window: Window) {
    // SAFETY: 창에 닫기 메시지를 넣는다. 없는 창이면 실패하고 끝난다.
    unsafe { PostMessageW(window.handle(), WM_CLOSE, 0, 0) };
}

/// 창이 소유한 대화상자(저장 확인). 없으면 None이다.
pub fn owned_dialog(window: Window) -> Option<Window> {
    pen_windows()
        .into_iter()
        .find(|candidate| class_name(*candidate) == WindowClass::DIALOG && owner_of(*candidate) == window.handle())
}

/// 대화상자에 취소(IDCANCEL)를 보낸다.
pub fn cancel_dialog(dialog: Window) {
    // SAFETY: 대화상자에 명령 메시지를 넣는다.
    unsafe { PostMessageW(dialog.handle(), WM_COMMAND, IDCANCEL as usize, 0) };
}

/// 버튼 찾기 상태.
struct Search<'a> {
    caption: &'a str,
    found: Option<Window>,
}

unsafe extern "system" fn match_button(
    child: HWND,
    data: LPARAM,
) -> i32 {
    // SAFETY: `data`는 `find_button`이 넘긴 `Search`의 주소이고, 열거가 끝날 때까지 살아 있다.
    let search = unsafe { &mut *(data as *mut Search) };
    let child = Window::from_handle(child);
    if class_name(child) == "Button" && title(child) == search.caption {
        search.found = Some(child);
        0
    } else {
        1
    }
}

/// 대화상자 안에서 글자가 `caption`인 버튼.
pub fn find_button(
    dialog: Window,
    caption: &str,
) -> Option<Window> {
    let mut search = Search { caption, found: None };
    // SAFETY: 콜백은 `search`만 읽고 쓰며, 열거는 반환 전에 끝난다.
    unsafe { EnumChildWindows(dialog.handle(), Some(match_button), &mut search as *mut Search as LPARAM) };
    search.found
}

pub fn click_button(button: Window) {
    // SAFETY: 버튼에 클릭 메시지를 넣는다.
    unsafe { PostMessageW(button.handle(), BM_CLICK, 0, 0) };
}
