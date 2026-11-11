//! `MEMORY_REQUEST`에 답하는 WSL 메모리 읽기: WSL 가상 머신의 메모리와 터미널별 고유 상주 메모리.
//!
//! 요청이 올 때만, 그리고 WSL 터미널이 있을 때만 읽는다(주기로 읽지 않는다). 읽는 일은 비싸서(`wsl.exe`는 수백 ms이고 배포판을 깨울 수 있다) 최근 10초 안의 값은 그대로 다시 쓴다.
//! 이 컴퓨터 전체 메모리는 앱이 직접 읽으므로 이 모듈은 WSL 몫만 맡는다. Windows가 아니면 WSL이 없어 항상 빈 답이다.
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use paddock_core::memory_guard::MemoryGuard;
use paddock_protocol::message::MemoryReport;

use crate::terminal::Registry;

/// 읽은 답과 읽는 중인지. 캐시가 만료되는 순간 요청이 겹쳐도 `wsl.exe`는 하나만 뜬다.
struct Cache {
    state: Mutex<Inner>,
    done: Condvar,
}

struct Inner {
    latest: Option<(Instant, MemoryReport)>,
    reading: bool,
    /// 마지막으로 로그에 남긴 결과(WSL 값이 있었는지). 바뀔 때만 로그를 남긴다.
    logged: Option<bool>,
}

static CACHE: Cache =
    Cache { state: Mutex::new(Inner { latest: None, reading: false, logged: None }), done: Condvar::new() };

/// 요청에 답할 내용. 최근 10초 안에 읽은 것이 있으면 그것이고, 다른 요청이 읽는 중이면 그 결과를 기다린다.
pub fn report(registry: &Registry) -> MemoryReport {
    report_via(&CACHE, || read(registry))
}

fn report_via(
    cache: &Cache,
    read: impl FnOnce() -> MemoryReport,
) -> MemoryReport {
    let fresh = Duration::from_millis(MemoryGuard::WSL_FRESH_MS);
    let mut inner = cache.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let cached = loop {
        match &inner.latest {
            Some((at, report)) if at.elapsed() < fresh => break Some(report.clone()),
            _ if inner.reading => inner = cache.done.wait(inner).unwrap_or_else(|poisoned| poisoned.into_inner()),
            _ => break None,
        }
    };
    cached.unwrap_or_else(|| {
        inner.reading = true;
        drop(inner);
        let report = read();
        let mut inner = cache.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.latest = Some((Instant::now(), report.clone()));
        inner.reading = false;
        let has_wsl = report.wsl.is_some();
        if inner.logged != Some(has_wsl) {
            inner.logged = Some(has_wsl);
            crate::log::event(&format!("memory: wsl memory {}", if has_wsl { "read" } else { "not available" }));
        }
        drop(inner);
        cache.done.notify_all();
        report
    })
}

#[cfg(windows)]
fn read(registry: &Registry) -> MemoryReport {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::terminal::marker_id;
    use paddock_core::memory_guard::WslMemory;

    // WSL 터미널이 있는 배포판마다 한 번씩 읽는다. WSL 터미널이 하나도 없으면 읽지 않는다(Memory를 켜 두었다고 WSL 가상 머신을 10초마다 깨우지 않는다).
    // 메모리 합계는 같은 가상 머신이라 첫 값을 쓴다.
    let mut distributions: Vec<Option<String>> = Vec::new();
    for target in registry.all().iter().filter_map(|terminal| terminal.probe_target()).filter_map(|target| target.wsl) {
        if !distributions.contains(&target.distribution) {
            distributions.push(target.distribution);
        }
    }
    let mut memory: Option<WslMemory> = None;
    let mut shells: BTreeSet<u32> = BTreeSet::new();
    let mut terminals: BTreeMap<u32, u64> = BTreeMap::new();
    for distribution in distributions {
        if let Some(parsed) = crate::probe::wsl::read_memory(distribution.as_deref()) {
            for (marker, bytes) in &parsed.terminals {
                if let Some(id) = marker_id(marker) {
                    *terminals.entry(id).or_insert(0) += bytes;
                }
            }
            shells.extend(parsed.shells.iter().filter_map(|marker| marker_id(marker)));
            if memory.is_none() {
                memory = Some(WslMemory {
                    total: parsed.total,
                    available: parsed.available,
                    swap_total: parsed.swap_total,
                    swap_used: parsed.swap_used,
                    ..WslMemory::default()
                });
            }
        }
    }
    MemoryReport {
        wsl: memory.map(|memory| WslMemory {
            detached: terminals.keys().filter(|id| !shells.contains(id)).copied().collect(),
            terminals,
            ..memory
        }),
    }
}

#[cfg(not(windows))]
fn read(registry: &Registry) -> MemoryReport {
    let _ = registry;
    MemoryReport::default()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn test_c_d76_requests_that_overlap_a_read_wait_for_it_and_share_its_result() {
        static LOCAL: Cache =
            Cache { state: Mutex::new(Inner { latest: None, reading: false, logged: None }), done: Condvar::new() };
        static N_READS: AtomicUsize = AtomicUsize::new(0);
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    report_via(&LOCAL, || {
                        N_READS.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(150));
                        MemoryReport::default()
                    })
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(N_READS.load(Ordering::SeqCst), 1, "four requests started one read");
        report_via(&LOCAL, || {
            N_READS.fetch_add(1, Ordering::SeqCst);
            MemoryReport::default()
        });
        assert_eq!(N_READS.load(Ordering::SeqCst), 1, "a fresh answer is reused");
    }
}
