//! Windows: 프로세스 목록(Toolhelp)과 명령줄 조회(`NtQueryInformationProcess`)로 셸의 앞쪽 프로그램을 찾는다.
//!
//! Windows에는 터미널의 "앞쪽 프로세스"가 없어, 셸의 자식 프로세스 중 가장 최근에 시작한 것을 앞쪽 프로그램으로 본다(`process_argv`).
//! 자식 프로세스(PowerShell 등)를 띄우지 않는다. 명령줄 조회는 ntdll의 공개되지 않은 기능이라 보호된 프로세스에서 실패할 수 있고,
//! 그때는 실행 파일 이름만으로 판정한다(`node.exe`로 뜬 codex는 `node`로만 보인다).
use std::collections::BTreeMap;
use std::sync::Mutex;

use paddock_core::agent_account::Runtime;
use paddock_core::process_argv::{ProcessItem, filetime_to_unix_ms, foreground_item, split_command_line};
use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, GetLastError, INVALID_HANDLE_VALUE, UNICODE_STRING};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use super::{ProbeTarget, Reading, wsl};
use crate::log;

/// 셸에서 따라 내려가 읽는 자손의 최대 깊이(셸 → 실행기 → 실행기 → 프로그램).
const N_MAX_DEPTH: usize = 3;

/// 명령줄 조회 실패를 기록 파일에 한 번씩만 남기려고 기억하는 프로세스 번호의 최대 수.
const N_REPORTED_FAILURES_MAX: usize = 256;

pub fn read(targets: &[ProbeTarget]) -> BTreeMap<u32, Reading> {
    let (wsl_targets, native_targets): (Vec<&ProbeTarget>, Vec<&ProbeTarget>) =
        targets.iter().partition(|target| target.wsl.is_some());
    let mut readings = BTreeMap::new();
    if !native_targets.is_empty() {
        let entries = snapshot();
        for target in native_targets {
            let items = family_of(&entries, target.shell_pid);
            if let Some(chosen) = foreground_item(&items, target.shell_pid) {
                readings.insert(
                    target.id,
                    Reading {
                        cwd: None,
                        argv: split_command_line(&chosen.command_line),
                        env: None,
                        runtime: Runtime::Native,
                        distribution: String::new(),
                        pid: Some(chosen.pid),
                        started_at: filetime_to_unix_ms(chosen.created_at),
                        session_record: None,
                    },
                );
            }
        }
    }
    readings.extend(wsl::read(&wsl_targets));
    readings
}

/// 프로세스 목록의 한 줄.
struct Entry {
    pid: u32,
    parent_pid: u32,
    exe_name: String,
}

/// 지금 떠 있는 모든 프로세스의 번호·부모·실행 파일 이름.
fn snapshot() -> Vec<Entry> {
    let mut entries = Vec::new();
    // SAFETY: 스냅샷 핸들은 이 함수 안에서만 쓰고 닫는다. PROCESSENTRY32W는 0으로 채운 뒤 크기를 적어 넘긴다.
    unsafe {
        let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if handle != INVALID_HANDLE_VALUE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(handle, &mut entry) != 0;
            while more {
                let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
                entries.push(Entry {
                    pid: entry.th32ProcessID,
                    parent_pid: entry.th32ParentProcessID,
                    exe_name: String::from_utf16_lossy(&entry.szExeFile[..length]),
                });
                more = Process32NextW(handle, &mut entry) != 0;
            }
            CloseHandle(handle);
        }
    }
    entries
}

/// 셸과 그 자손(`N_MAX_DEPTH` 단계까지)의 명령줄·시작 시각.
fn family_of(
    entries: &[Entry],
    shell_pid: u32,
) -> Vec<ProcessItem> {
    let mut family: Vec<&Entry> = entries.iter().filter(|entry| entry.pid == shell_pid).collect();
    let mut frontier: Vec<u32> = vec![shell_pid];
    for _ in 0..N_MAX_DEPTH {
        let children: Vec<&Entry> = entries
            .iter()
            .filter(|entry| entry.pid != entry.parent_pid && frontier.contains(&entry.parent_pid))
            .filter(|entry| !family.iter().any(|known| known.pid == entry.pid))
            .collect();
        frontier = children.iter().map(|entry| entry.pid).collect();
        family.extend(children);
    }
    family
        .into_iter()
        .map(|entry| {
            let details = process_details(entry.pid);
            ProcessItem {
                pid: entry.pid,
                parent_pid: entry.parent_pid,
                command_line: details.0.unwrap_or_else(|| entry.exe_name.clone()),
                created_at: details.1,
            }
        })
        .collect()
}

/// 프로세스의 (명령줄, 시작 시각). 열 수 없거나 조회에 실패한 값은 None·0이다.
fn process_details(pid: u32) -> (Option<String>, u64) {
    // SAFETY: 핸들은 이 함수 안에서만 쓰고 닫는다.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            note_failure(pid, &format!("process could not be opened (os error {})", GetLastError()));
            (None, 0)
        } else {
            let details = (command_line_of(handle, pid), created_at_of(handle));
            CloseHandle(handle);
            details
        }
    }
}

/// 프로세스의 시작 시각(FILETIME을 u64로). 크기 비교에만 쓴다.
///
/// # Safety
/// `handle`은 `PROCESS_QUERY_LIMITED_INFORMATION`으로 연 유효한 프로세스 핸들이어야 한다.
unsafe fn created_at_of(handle: windows_sys::Win32::Foundation::HANDLE) -> u64 {
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: 호출자가 핸들의 유효성을 보장하고, 네 FILETIME은 이 함수의 지역 변수다.
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } != 0;
    if ok { (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime) } else { 0 }
}

/// 명령줄 조회 실패를 기록 파일에 한 줄 남긴다(같은 프로세스 번호는 한 번만). 사유와 프로세스 번호만 적고 명령줄은 적지 않는다.
/// 이 경우 호출자는 실행 파일 이름으로 프로그램을 판정한다.
fn note_failure(
    pid: u32,
    reason: &str,
) {
    static REPORTED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
    let mut reported = REPORTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !reported.contains(&pid) {
        // 오래 도는 데몬에서 목록이 끝없이 자라지 않게 오래된 것부터 버린다.
        if reported.len() >= N_REPORTED_FAILURES_MAX {
            reported.remove(0);
        }
        reported.push(pid);
        log::event(&format!("probe: command line of pid {pid} unavailable ({reason}); using the executable name"));
    }
}

/// 프로세스의 명령줄. 조회가 거부되면 None이고 사유를 기록 파일에 남긴다.
///
/// # Safety
/// `handle`은 `PROCESS_QUERY_LIMITED_INFORMATION`으로 연 유효한 프로세스 핸들이어야 한다.
unsafe fn command_line_of(
    handle: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
) -> Option<String> {
    let mut length = 0u32;
    // SAFETY: 크기 0 버퍼로 필요한 크기만 묻는다(실패 코드와 함께 length가 채워진다).
    unsafe { NtQueryInformationProcess(handle, ProcessCommandLineInformation, std::ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        note_failure(pid, "size query returned 0");
        None
    } else {
        // UNICODE_STRING이 8바이트 정렬을 요구하므로 u64 버퍼로 잡는다.
        let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
        // SAFETY: 버퍼는 length바이트 이상이다.
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                ProcessCommandLineInformation,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        };
        if status < 0 {
            note_failure(pid, &format!("NtQueryInformationProcess status {status:#x}"));
            None
        } else {
            // SAFETY: 성공하면 버퍼 앞에 UNICODE_STRING이 있다.
            let unicode = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
            if unicode.Buffer.is_null() || unicode.Length == 0 {
                // 빈 명령줄(null 포인터)은 슬라이스로 만들면 정의되지 않은 동작이다.
                note_failure(pid, "command line is empty");
                None
            } else {
                // SAFETY: Buffer는 null이 아니고 같은 버퍼 안을 가리키며 Length바이트(UTF-16 글자 Length/2개)가 유효하다.
                let text = unsafe { std::slice::from_raw_parts(unicode.Buffer, usize::from(unicode.Length) / 2) };
                Some(String::from_utf16_lossy(text))
            }
        }
    }
}
