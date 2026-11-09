//! 자동 점검 빌드의 메모리 할당 계수기: Rust 쪽이 지금 쥐고 있는 바이트의 합을 센다.
//!
//! 프로세스의 Private 바이트에는 Rust 코드가 받은 메모리와 그래픽 드라이버가 따로 잡은 메모리가 섞여 있다. 드라이버는 Rust 할당기를
//! 거치지 않으므로, 이 합이 제자리로 돌아오는데 Private만 남으면 남는 몫은 Rust 쪽이 아니라 드라이버·운영체제 쪽에 있다.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

/// 이 크기(바이트) 이상의 블록은 크기 칸(MiB 단위)별 살아 있는 개수를 센다.
const BIG_BYTES: usize = 1 << 20;
/// 칸 수: 0..=62 MiB, 마지막 칸은 그보다 큰 블록.
const N_CLASSES: usize = 64;
static BIG_LIVE: [AtomicUsize; N_CLASSES] = [const { AtomicUsize::new(0) }; N_CLASSES];

/// 이 크기 이상의 블록은 할당한 곳(호출 스택)도 적어 둔다.
const SITE_BYTES: usize = 2 << 20;
const N_SITES: usize = 128;
const N_FRAMES: usize = 14;

/// 살아 있는 큰 블록 하나의 기록: 주소·크기·할당 때의 호출 스택(주소만).
struct Site {
    pointer: AtomicUsize,
    size: AtomicUsize,
    frames: [AtomicUsize; N_FRAMES],
}

static SITES: [Site; N_SITES] = [const {
    Site { pointer: AtomicUsize::new(0), size: AtomicUsize::new(0), frames: [const { AtomicUsize::new(0) }; N_FRAMES] }
}; N_SITES];

fn class_of(size: usize) -> usize {
    (size >> 20).min(N_CLASSES - 1)
}

/// 큰 블록이 생겼다.
fn note_alloc(
    pointer: *mut u8,
    size: usize,
) {
    if size >= BIG_BYTES {
        BIG_LIVE[class_of(size)].fetch_add(1, Ordering::Relaxed);
    }
    if size >= SITE_BYTES {
        let address = pointer as usize;
        for site in &SITES {
            if site.pointer.compare_exchange(0, address, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                site.size.store(size, Ordering::Relaxed);
                let mut frames = [std::ptr::null_mut::<core::ffi::c_void>(); N_FRAMES];
                #[cfg(windows)]
                // SAFETY: 크기 `N_FRAMES`의 배열에 호출 스택의 반환 주소를 받는다. 할당하지 않는다.
                let n =
                    unsafe { RtlCaptureStackBackTrace(1, N_FRAMES as u32, frames.as_mut_ptr(), std::ptr::null_mut()) };
                #[cfg(not(windows))]
                let n = 0u16;
                for (slot, frame) in site.frames.iter().zip(frames.iter()).take(n as usize) {
                    slot.store(*frame as usize, Ordering::Relaxed);
                }
                for slot in site.frames.iter().skip(n as usize) {
                    slot.store(0, Ordering::Relaxed);
                }
                break;
            }
        }
    }
}

/// 큰 블록이 없어졌다.
fn note_free(
    pointer: *mut u8,
    size: usize,
) {
    if size >= BIG_BYTES {
        BIG_LIVE[class_of(size)].fetch_sub(1, Ordering::Relaxed);
    }
    if size >= SITE_BYTES {
        let address = pointer as usize;
        for site in &SITES {
            if site.pointer.compare_exchange(address, 0, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                break;
            }
        }
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn RtlCaptureStackBackTrace(
        frames_to_skip: u32,
        frames_to_capture: u32,
        back_trace: *mut *mut core::ffi::c_void,
        back_trace_hash: *mut u32,
    ) -> u16;
    fn GetModuleHandleW(module_name: *const u16) -> isize;
}

/// 살아 있는 1 MiB 이상 블록의 크기 칸별 개수 한 줄(`4x1`은 4~5 MiB 블록 1개)과, 2 MiB 이상 블록의 할당 호출 스택(실행 파일 기준 오프셋).
#[cfg(windows)]
pub fn big_report() -> (String, Vec<String>) {
    let classes: Vec<String> = BIG_LIVE
        .iter()
        .enumerate()
        .filter_map(|(class, count)| {
            let n = count.load(Ordering::Relaxed);
            (n > 0).then(|| format!("{class}x{n}"))
        })
        .collect();
    // SAFETY: 인자가 널이면 이 프로세스의 실행 파일 모듈 핸들이다.
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let sites = SITES
        .iter()
        .filter(|site| site.pointer.load(Ordering::Acquire) != 0)
        .map(|site| {
            let frames: Vec<String> = site
                .frames
                .iter()
                .map(|frame| frame.load(Ordering::Relaxed))
                .take_while(|frame| *frame != 0)
                .map(|frame| format!("{:#x}", frame.wrapping_sub(base)))
                .collect();
            format!("size={} frames={}", site.size.load(Ordering::Relaxed), frames.join(","))
        })
        .collect();
    (classes.join(","), sites)
}

/// 시스템 할당기에 맡기되 살아 있는 바이트 수를 센다.
pub struct CountingAlloc;

// SAFETY: 모든 호출을 `System`에 그대로 넘기고 계수만 더한다.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        // SAFETY: 호출자가 `GlobalAlloc::alloc`의 조건을 지킨다.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            note_alloc(pointer, layout.size());
        }
        pointer
    }

    unsafe fn dealloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
    ) {
        // SAFETY: 호출자가 `GlobalAlloc::dealloc`의 조건을 지킨다.
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        note_free(pointer, layout.size());
    }

    unsafe fn alloc_zeroed(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        // SAFETY: 호출자가 `GlobalAlloc::alloc_zeroed`의 조건을 지킨다.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            note_alloc(pointer, layout.size());
        }
        pointer
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        // SAFETY: 호출자가 `GlobalAlloc::realloc`의 조건을 지킨다.
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            LIVE_BYTES.fetch_add(new_size, Ordering::Relaxed);
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            note_free(pointer, layout.size());
            note_alloc(moved, new_size);
        }
        moved
    }
}

/// Rust 쪽이 지금 쥐고 있는 바이트 수.
pub fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}
