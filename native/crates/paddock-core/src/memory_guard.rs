//! 시스템 메모리 게이지와 90% 경고의 순수 규칙: 어느 쪽(이 컴퓨터·WSL)이 더 빡빡한지, 가장 큰 터미널, 고칠 설정 안내, 경고 문장과 한 번만 울리는 규칙.
//!
//! WSL은 자기 가상 머신 안에서 먼저 바닥나므로 Windows 전체 사용률만으로는 그 위기를 못 본다. 반대로 Windows가 먼저 바닥나는 설정도 있어 높은 쪽을 쓴다.
//! 읽기(운영체제 호출)는 호출자가 하고, 이 모듈은 읽은 값에서 판정과 문장만 만든다.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 메모리 경고 기준.
pub struct MemoryGuard;

impl MemoryGuard {
    /// 이 퍼센트 이상이 되면 한 번 경고한다.
    pub const WARN_PERCENT: u32 = 90;
    /// 경고한 뒤 이 퍼센트 아래로 내려가야 다시 경고할 수 있다.
    pub const REARM_PERCENT: u32 = 80;
    /// WSL 상한을 빼고도 Windows에 남아야 하는 최소 메모리(Windows와 앱 자신이 쓰는 몫).
    pub const MIN_WINDOWS_HEADROOM_BYTES: u64 = 8 * 1024 * 1024 * 1024;
    /// WSL 값을 읽은 뒤 다시 읽지 않고 쓰는 시간(ms).
    pub const WSL_FRESH_MS: u64 = 10_000;
    /// 이 만큼 이상 swap을 쓰면 느려진 이유로 알린다.
    pub const SWAP_NOTICE_BYTES: u64 = 512 * 1024 * 1024;
}

/// WSL 가상 머신에서 읽은 메모리. 값은 모두 바이트이고, `terminals`는 터미널 번호별 고유 상주 메모리(상주 − 공유)다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WslMemory {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub terminals: BTreeMap<u32, u64>,
    /// 셸이 이미 닫혔는데 표지를 물려받은 프로세스(에뮬레이터·개발 서버 등)만 남은 터미널 번호.
    pub detached: Vec<u32>,
}

/// 사용률(%)을 정수로. 전체 용량을 모르면 None이다.
///
/// Examples
/// --------
/// | total | free | 결과 |
/// |-------|------|------|
/// | 32    | 16   | 50   |
/// | 26    | 2    | 92   |
/// | 0     | 0    | None |
pub fn memory_percent(
    total: u64,
    free: u64,
) -> Option<u32> {
    (total > 0).then(|| {
        let used = total.saturating_sub(free) as f64;
        (used / total as f64 * 100.0).round() as u32
    })
}

/// 사용률이 가리키는 쪽.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// 이 컴퓨터(Windows·Linux·macOS 전체 메모리).
    System,
    Wsl,
}

/// 이 컴퓨터와 WSL 중 더 빡빡한 쪽의 사용률과 그 출처. WSL 값이 없으면 이 컴퓨터 값이고, 둘 다 모르면 `(None, System)`이다.
///
/// Examples
/// --------
/// | 이 컴퓨터(total, free) | WSL(total, available) | 결과          |
/// |------------------------|-----------------------|---------------|
/// | 32, 16                 | 26, 2                 | 92 · `Wsl`    |
/// | 32, 2                  | 26, 20                | 94 · `System` |
/// | 32, 16                 | 없음                  | 50 · `System` |
pub fn tightest_memory(
    system: Option<(u64, u64)>,
    wsl: Option<&WslMemory>,
) -> (Option<u32>, Source) {
    let system_percent = system.and_then(|(total, free)| memory_percent(total, free));
    let wsl_percent = wsl.and_then(|wsl| memory_percent(wsl.total, wsl.available));
    match (system_percent, wsl_percent) {
        (_, Some(wsl)) if system_percent.is_none_or(|system| wsl > system) => (Some(wsl), Source::Wsl),
        (system, _) => (system, Source::System),
    }
}

/// 사용자가 고칠 설정 문제.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// WSL 상한을 빼고 Windows에 남는 메모리가 8 GiB보다 적다.
    WslShare,
    /// WSL에 swap이 없어 메모리가 넘치면 느려지는 대신 프로세스가 바로 죽는다.
    NoSwap,
}

impl Advice {
    /// 설정 파일(`.wslconfig`)에서 고칠 곳을 알리는 문장.
    pub fn text(self) -> &'static str {
        match self {
            Self::WslShare => "WSL leaves Windows under 8 GB; lower memory= in .wslconfig so Windows keeps room.",
            Self::NoSwap => {
                "WSL has no swap; set swap= in .wslconfig so a spike slows things down instead of killing processes."
            }
        }
    }
}

/// 메모리가 바닥나기 전에 고칠 수 있는 설정 문제. 값은 실제로 읽은 바이트이고, 읽지 못한 값은 문제로 세지 않는다.
///
/// Examples
/// --------
/// | 이 컴퓨터 total | WSL total | swap total | 결과        |
/// |-----------------|-----------|------------|-------------|
/// | 32 GiB          | 26 GiB    | 16 GiB     | `WslShare`  |
/// | 32 GiB          | 20 GiB    | 0          | `NoSwap`    |
/// | 32 GiB          | 없음      | 없음       | (없음)      |
pub fn memory_advice(
    system_total: Option<u64>,
    wsl: Option<&WslMemory>,
) -> Vec<Advice> {
    let mut advice = Vec::new();
    if let (Some(system), Some(wsl)) = (system_total, wsl)
        && system > 0
        && wsl.total > 0
        && system.saturating_sub(wsl.total) < MemoryGuard::MIN_WINDOWS_HEADROOM_BYTES
    {
        advice.push(Advice::WslShare);
    }
    if wsl.is_some_and(|wsl| wsl.total > 0 && wsl.swap_total == 0) {
        advice.push(Advice::NoSwap);
    }
    advice
}

/// 터미널 번호별 메모리(바이트)에서 가장 큰 항목. 비었거나 모두 0이면 None이고, 크기가 같으면 번호가 작은 쪽이다.
pub fn heaviest_terminal(terminals: &BTreeMap<u32, u64>) -> Option<(u32, u64)> {
    terminals.iter().filter(|(_, bytes)| **bytes > 0).fold(None, |best, (id, bytes)| match best {
        Some((_, best_bytes)) if best_bytes >= *bytes => best,
        _ => Some((*id, *bytes)),
    })
}

/// 가장 큰 터미널이 누구의 것인지.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    /// 이 창의 터미널(화면에 보이는 이름).
    ThisWindow(String),
    /// 셸이 이미 닫혔는데 프로세스가 남았다.
    Closed,
    /// 이 창에 없는 터미널(다른 창).
    Elsewhere,
}

/// 게이지 말풍선·경고 문장과 경고에 붙일 단추.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDetails {
    /// 상태 줄 말풍선(`… Hide it in Quick settings.`는 호출자가 덧붙인다).
    pub summary: String,
    /// 경고 문장: 상태 한 문장과 할 일 한 문장.
    pub warning: String,
    /// 경고에 붙일 단추: 가장 큰 터미널이 이 창에 있을 때만 그 터미널 번호와 `Go to <이름>`.
    pub target: Option<(u32, String)>,
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// 게이지 말풍선과 경고 문장을 만든다.
///
/// 가장 큰 터미널은 어느 쪽이 빡빡하든 보인다 — Windows가 먼저 차는 설정에서도 WSL 안 어느 터미널이 큰지가 사용자가 바로 줄일 수 있는 정보다.
/// 알림은 상태 한 문장과 할 일 한 문장만 말한다. swap·설정 안내는 말풍선에만 둔다.
///
/// Examples
/// --------
/// `name_in_window`는 터미널 번호가 이 창의 터미널이면 그 이름을 돌려준다.
///
/// | 출처 | 가장 큰 터미널          | 경고 문장                                                                |
/// |------|-------------------------|--------------------------------------------------------------------------|
/// | WSL  | 이 창 `build` 3.1 GB    | `WSL memory is 92% full. build uses 3.1 GB. Stop its work or close it.`  |
/// | 이 컴퓨터 | 닫힌 터미널 0.9 GB | `System memory is 92% full. A closed terminal left processes using 0.9 GB. Stop them.` |
/// | 이 컴퓨터 | 없음              | `System memory is 92% full. Close apps you are not using.`               |
pub fn memory_details(
    percent: u32,
    source: Source,
    wsl: Option<&WslMemory>,
    system_total: Option<u64>,
    name_in_window: &dyn Fn(u32) -> Option<String>,
) -> MemoryDetails {
    let scope = if source == Source::Wsl { "WSL memory" } else { "System memory" };
    let heaviest = wsl.and_then(|wsl| heaviest_terminal(&wsl.terminals));
    // 이 창의 터미널이면 그 이름, 아니면 셸이 닫혀 프로세스만 남은 것인지 다른 창의 터미널인지로 가른다.
    let owner = heaviest.map(|(id, _)| {
        let owner = match name_in_window(id) {
            Some(name) => Owner::ThisWindow(name),
            None if wsl.is_some_and(|wsl| wsl.detached.contains(&id)) => Owner::Closed,
            None => Owner::Elsewhere,
        };
        (id, owner)
    });
    let size = heaviest.map(|(_, bytes)| gigabytes(bytes)).unwrap_or_default();
    let largest = match &owner {
        Some((_, Owner::ThisWindow(name))) => format!(" Largest WSL terminal: {name} ({size} GB)."),
        Some((_, Owner::Closed)) => {
            format!(" Largest WSL terminal: leftover processes of a closed terminal ({size} GB).")
        }
        Some((_, Owner::Elsewhere)) => format!(" Largest WSL terminal: a terminal in another window ({size} GB)."),
        None => String::new(),
    };
    let swapping = wsl
        .filter(|wsl| wsl.swap_used >= MemoryGuard::SWAP_NOTICE_BYTES)
        .map(|wsl| format!(" WSL is swapping {} GB, which slows everything.", gigabytes(wsl.swap_used)))
        .unwrap_or_default();
    let advice = memory_advice(system_total, wsl).into_iter().map(Advice::text).collect::<Vec<_>>().join(" ");
    let advice = if advice.is_empty() { String::new() } else { format!(" {advice}") };
    let action = match &owner {
        Some((_, Owner::ThisWindow(name))) => format!("{name} uses {size} GB. Stop its work or close it."),
        Some((_, Owner::Closed)) => format!("A closed terminal left processes using {size} GB. Stop them."),
        Some((_, Owner::Elsewhere)) => {
            format!("A terminal in another window uses {size} GB. Stop its work or close it there.")
        }
        None if source == Source::Wsl => "Close terminals you are not using.".to_string(),
        None => "Close apps you are not using.".to_string(),
    };
    MemoryDetails {
        summary: format!("{scope} in use: {percent}%.{largest}{swapping}{advice}"),
        warning: format!("{scope} is {percent}% full. {action}"),
        target: match owner {
            Some((id, Owner::ThisWindow(name))) => Some((id, format!("Go to {name}"))),
            _ => None,
        },
    }
}

/// 경고를 낼지와 다음 `warned` 상태. 90% 이상이 처음 되면 한 번 내고, 80% 아래로 내려가야 다시 낼 수 있다(경계에서 오르내려도 반복하지 않는다).
///
/// Examples
/// --------
/// | 사용률 | warned | 결과                 |
/// |--------|--------|----------------------|
/// | 92     | false  | 경고, warned=true    |
/// | 95     | true   | 경고 없음, true      |
/// | 85     | true   | 경고 없음, true      |
/// | 70     | true   | 경고 없음, false     |
pub fn warn_step(
    percent: u32,
    warned: bool,
) -> (bool, bool) {
    if percent >= MemoryGuard::WARN_PERCENT && !warned {
        (true, true)
    } else if percent < MemoryGuard::REARM_PERCENT {
        (false, false)
    } else {
        (false, warned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn wsl(
        total: u64,
        available: u64,
    ) -> WslMemory {
        WslMemory { total, available, swap_total: 16 * GIB, ..WslMemory::default() }
    }

    #[test]
    fn test_c_d76_the_tighter_of_the_computer_and_wsl_wins_and_a_missing_side_does_not() {
        assert_eq!(tightest_memory(Some((32, 16)), Some(&wsl(26, 2))), (Some(92), Source::Wsl));
        assert_eq!(tightest_memory(Some((32, 2)), Some(&wsl(26, 20))), (Some(94), Source::System));
        assert_eq!(tightest_memory(Some((32, 16)), None), (Some(50), Source::System));
        assert_eq!(tightest_memory(None, Some(&wsl(26, 2))), (Some(92), Source::Wsl));
        assert_eq!(tightest_memory(None, None), (None, Source::System));
    }

    #[test]
    fn test_c_d76_advice_names_only_problems_that_were_actually_read() {
        let with_swap = |total: u64, swap: u64| WslMemory { total, swap_total: swap, ..WslMemory::default() };
        assert_eq!(memory_advice(Some(32 * GIB), Some(&with_swap(26 * GIB, 16 * GIB))), vec![Advice::WslShare]);
        assert_eq!(memory_advice(Some(32 * GIB), Some(&with_swap(20 * GIB, 0))), vec![Advice::NoSwap]);
        assert_eq!(memory_advice(Some(32 * GIB), None), Vec::new());
        assert_eq!(memory_advice(None, Some(&with_swap(26 * GIB, 16 * GIB))), Vec::new());
    }

    #[test]
    fn test_c_d76_the_heaviest_terminal_ignores_zero_and_breaks_ties_by_the_smaller_number() {
        let map = |entries: &[(u32, u64)]| entries.iter().copied().collect::<BTreeMap<u32, u64>>();
        assert_eq!(heaviest_terminal(&map(&[(1, 10), (2, 30), (3, 20)])), Some((2, 30)));
        assert_eq!(heaviest_terminal(&map(&[(1, 0), (2, 0)])), None);
        assert_eq!(heaviest_terminal(&map(&[(4, 5), (2, 5)])), Some((2, 5)));
        assert_eq!(heaviest_terminal(&map(&[])), None);
    }

    #[test]
    fn test_c_d76_warning_sentences_name_the_owner_of_the_biggest_terminal_in_five_ways() {
        let mut memory = wsl(26 * GIB, 2 * GIB);
        memory.terminals.insert(7, 3 * GIB + GIB / 10);
        let details = |owner: Owner, source: Source, memory: Option<&WslMemory>| {
            let mut memory = memory.cloned();
            if owner == Owner::Closed
                && let Some(memory) = memory.as_mut()
            {
                memory.detached.push(7);
            }
            let name = match &owner {
                Owner::ThisWindow(name) => Some(name.clone()),
                _ => None,
            };
            memory_details(92, source, memory.as_ref(), Some(64 * GIB), &move |_| name.clone())
        };
        let this = details(Owner::ThisWindow("build".to_string()), Source::Wsl, Some(&memory));
        assert_eq!(this.warning, "WSL memory is 92% full. build uses 3.1 GB. Stop its work or close it.");
        assert_eq!(this.target, Some((7, "Go to build".to_string())));
        assert!(
            this.summary.starts_with("WSL memory in use: 92%. Largest WSL terminal: build (3.1 GB)."),
            "{}",
            this.summary
        );
        let closed = details(Owner::Closed, Source::System, Some(&memory));
        assert_eq!(
            closed.warning,
            "System memory is 92% full. A closed terminal left processes using 3.1 GB. Stop them."
        );
        assert_eq!(closed.target, None);
        let elsewhere = details(Owner::Elsewhere, Source::System, Some(&memory));
        assert_eq!(
            elsewhere.warning,
            "System memory is 92% full. A terminal in another window uses 3.1 GB. Stop its work or close it there."
        );
        let none_wsl = details(Owner::Closed, Source::Wsl, Some(&wsl(26 * GIB, 2 * GIB)));
        assert_eq!(none_wsl.warning, "WSL memory is 92% full. Close terminals you are not using.");
        let none_system = details(Owner::Closed, Source::System, None);
        assert_eq!(none_system.warning, "System memory is 92% full. Close apps you are not using.");
    }

    #[test]
    fn test_c_d76_the_tooltip_adds_swap_and_settings_advice_that_the_warning_leaves_out() {
        let memory =
            WslMemory { total: 20 * GIB, available: 2 * GIB, swap_total: 0, swap_used: GIB, ..WslMemory::default() };
        let details = memory_details(92, Source::Wsl, Some(&memory), Some(32 * GIB), &|_| None);
        assert!(details.summary.contains("WSL is swapping 1.0 GB, which slows everything."), "{}", details.summary);
        assert!(details.summary.contains("set swap= in .wslconfig"), "{}", details.summary);
        assert!(!details.warning.contains("swap"));
    }

    #[test]
    fn test_c_d76_a_warning_fires_once_at_ninety_and_rearms_only_below_eighty() {
        assert_eq!(warn_step(92, false), (true, true));
        assert_eq!(warn_step(95, true), (false, true));
        assert_eq!(warn_step(85, true), (false, true), "between 80 and 90 stays warned");
        assert_eq!(warn_step(70, true), (false, false));
        assert_eq!(warn_step(85, false), (false, false));
    }

    #[test]
    fn test_c_d76_percent_rounds_like_the_old_app() {
        assert_eq!(memory_percent(3, 1), Some(67));
        assert_eq!(memory_percent(200, 1), Some(100), "99.5 rounds up");
        assert_eq!(memory_percent(0, 0), None);
    }
}
