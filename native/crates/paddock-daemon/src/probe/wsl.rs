//! Windows의 WSL 터미널: 배포판마다 `wsl.exe -d <배포판> -e sh -c <스크립트>`를 한 번 실행해 표지가 붙은 셸을 모두 읽는다.
//!
//! `wsl.exe` 호출은 비싸고(수백 ms) 배포판을 깨울 수 있으므로, 조사할 WSL 터미널이 있을 때만 배포판당 한 번 부른다.
//! 호출할 때마다 기록 파일에 한 줄을 남겨 호출 횟수를 셀 수 있다.
use std::collections::BTreeMap;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use paddock_core::agent_account::Runtime;
use paddock_core::wsl_probe::{WslTerminal, parse, script};

use super::{ProbeTarget, Reading};
use crate::log;

/// `wsl.exe`가 이 시간 안에 끝나지 않으면 죽이고 이번 조사는 건너뛴다.
///
/// 조사는 판정 틱 스레드 하나가 순서대로 하므로, `wsl.exe`가 멈추면 이 시간만큼 모든 터미널의 판정이 늦어진다.
/// 정상 호출은 수백 ms이므로 시간을 짧게 잡아 늦어지는 폭을 줄인다(멈춘 호출을 별도 스레드로 빼는 대신 감수한 선택).
const WSL_TIMEOUT: Duration = Duration::from_secs(4);

/// 자식 프로세스의 콘솔 창을 만들지 않는 생성 옵션(`CREATE_NO_WINDOW`).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn read(targets: &[&ProbeTarget]) -> BTreeMap<u32, Reading> {
    let mut distributions: Vec<Option<String>> = Vec::new();
    for target in targets {
        if let Some(wsl) = &target.wsl
            && !distributions.contains(&wsl.distribution)
        {
            distributions.push(wsl.distribution.clone());
        }
    }
    let mut readings = BTreeMap::new();
    for distribution in distributions {
        let found = run(distribution.as_deref()).map(|output| parse(&output)).unwrap_or_default();
        for target in targets {
            if let Some(wsl) = target.wsl.as_ref().filter(|wsl| wsl.distribution == distribution)
                && let Some(terminal) = found.get(&wsl.marker)
            {
                readings.insert(target.id, reading_of(terminal));
            }
        }
    }
    readings
}

fn reading_of(terminal: &WslTerminal) -> Reading {
    Reading {
        cwd: (!terminal.cwd.is_empty()).then(|| terminal.cwd.clone()),
        argv: terminal.argv.clone(),
        env: Some(terminal.env.clone()),
        runtime: Runtime::Wsl,
        distribution: terminal.distribution.clone(),
        pid: terminal.pid.trim().parse().ok(),
        started_at: None,
        session_record: Some(terminal.session_record.clone()).filter(|record| !record.is_empty()),
    }
}

/// 배포판 안에서 스크립트를 실행해 표준 출력을 돌려준다. 실행하지 못하거나 시간이 지나면 None이다.
fn run(distribution: Option<&str>) -> Option<String> {
    log::event(&format!("probe: wsl.exe read (distribution {})", distribution.unwrap_or("default")));
    let mut command = Command::new("wsl.exe");
    if let Some(distribution) = distribution {
        command.args(["-d", distribution]);
    }
    command
        .args(["-e", "sh", "-c", &script()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    match receiver.recv_timeout(WSL_TIMEOUT) {
        Ok(bytes) => {
            let _ = child.wait();
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        Err(_) => {
            log::event("probe: wsl.exe did not answer in time, killed");
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}
