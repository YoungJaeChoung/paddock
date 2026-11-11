//! 같은 순간의 프로세스 메모리와 그래픽 메모리 사용량 한 줄(Rust 쪽 보유량 포함).
//!
//! 바깥 실행기의 성능 카운터는 표본이 한 칸 늦어, 닫은 뒤 남는 몫이 이 앱(GPUI)이 쥔 것인지 그래픽 드라이버가 쥔 것인지
//! 가르지 못했다. 이 프로세스가 직접 읽으면 Private 바이트와 그래픽 메모리를 같은 호출 안에서 얻는다.
//! 그래픽 메모리는 DXGI `QueryVideoMemoryInfo`가 알려 주는 이 프로세스의 어댑터별 사용량이다
//! (local = 그래픽 카드 쪽, non-local = 시스템 메모리를 그래픽이 쓰는 몫).
use std::fmt::Write;

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory1,
};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::core::Interface;

const MEGABYTE: f64 = 1024.0 * 1024.0;

/// 이 프로세스의 Private 바이트·작업 집합과 어댑터별 그래픽 메모리 사용량(MB)을 한 줄로 만든다.
///
/// 읽지 못한 값은 `n/a`다. 소프트웨어 어댑터(기본 렌더 드라이버)는 건너뛴다.
pub fn sample_line(label: &str) -> String {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: 현재 프로세스 핸들과 크기를 맞춘 구조체를 넘긴다.
    let process = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
    };
    let mut line = match process {
        Ok(()) => format!(
            "{label} PrivateMB={:.1} WorkingSetMB={:.1} RustLiveMB={:.1}",
            counters.PrivateUsage as f64 / MEGABYTE,
            counters.WorkingSetSize as f64 / MEGABYTE,
            super::alloc::live_bytes() as f64 / MEGABYTE
        ),
        Err(_) => format!(
            "{label} PrivateMB=n/a WorkingSetMB=n/a RustLiveMB={:.1}",
            super::alloc::live_bytes() as f64 / MEGABYTE
        ),
    };
    let (n_buried, n_drained) = crate::files::images::audit::counts();
    let _ = write!(
        line,
        " Buried={n_buried} Drained={n_drained} LastFullStrong={}",
        crate::files::images::audit::last_full_strong()
    );
    // SAFETY: 인자 없는 DXGI 공장 생성이다.
    match unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
        Ok(factory) => {
            let mut index = 0;
            // SAFETY: 어댑터 번호를 하나씩 올려 EnumAdapters1이 오류를 낼 때까지 읽는다.
            while let Ok(adapter) = unsafe { factory.EnumAdapters1(index) } {
                index += 1;
                // SAFETY: 위에서 얻은 어댑터에 대한 조회뿐이다.
                let desc = unsafe { adapter.GetDesc1() };
                let software = desc.as_ref().is_ok_and(|desc| desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0);
                if software {
                    continue;
                }
                let name = desc
                    .map(|desc| String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').replace(' ', "_"))
                    .unwrap_or_default();
                let (local, non_local) = match adapter.cast::<IDXGIAdapter3>() {
                    Ok(adapter3) => (
                        usage(&adapter3, DXGI_MEMORY_SEGMENT_GROUP_LOCAL.0),
                        usage(&adapter3, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL.0),
                    ),
                    Err(_) => ("n/a".to_string(), "n/a".to_string()),
                };
                let _ = write!(line, " gpu{}={name} DxgiLocalMB={local} DxgiNonLocalMB={non_local}", index - 1);
            }
        }
        Err(_) => line.push_str(" DxgiLocalMB=n/a DxgiNonLocalMB=n/a"),
    }
    let _ = write!(line, " {}", super::draw_counts());
    let (classes, sites) = super::alloc::big_report();
    let _ = write!(line, " Big={classes}");
    // 2 MiB 이상 블록의 할당 호출 스택은 따로 적는다(`bigsites.log`): 라벨마다 살아 있는 블록 전부.
    if let Some(out) = super::OUT.get()
        && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(out.join("bigsites.log"))
    {
        use std::io::Write as _;
        for site in &sites {
            let _ = writeln!(file, "{label} {site}");
        }
    }
    line
}

/// 어댑터의 한 메모리 구역에서 이 프로세스가 쓰는 양(MB). 읽지 못하면 `n/a`.
fn usage(
    adapter: &IDXGIAdapter3,
    segment_group: i32,
) -> String {
    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    // SAFETY: 노드 0의 구역 사용량을 받을 구조체를 넘긴다.
    let read = unsafe {
        adapter.QueryVideoMemoryInfo(
            0,
            windows::Win32::Graphics::Dxgi::DXGI_MEMORY_SEGMENT_GROUP(segment_group),
            &mut info,
        )
    };
    match read {
        Ok(()) => format!("{:.1}", info.CurrentUsage as f64 / MEGABYTE),
        Err(_) => "n/a".to_string(),
    }
}
