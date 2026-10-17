//! 연결 하나의 보내기 통로. 쓰기 스레드가 소켓에 쓰기 전까지 데몬 메모리에 쌓이는 바이트 수에 상한을 둔다.
//!
//! 클라이언트가 소켓을 읽지 않아도 터미널 출력은 계속 나오므로, 통로가 무한하면 데몬 메모리가 끝없이 는다.
//! 상한을 넘은 출력은 버리고 연결에 "다시 맞춰야 함" 표시만 남긴다. 링 버퍼가 출력의 정본이라
//! 버린 출력은 링 재생으로 되살릴 수 있다. 쓰기 스레드가 쌓인 프레임을 모두 쓰면(클라이언트가 다시 읽기 시작한 것이다)
//! 등록된 훅이 불려 호출자가 재생을 다시 보낼 수 있다.
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel as mpsc_channel};
use std::sync::{Arc, Mutex};

use paddock_protocol::frame::Frame;

/// 연결 하나에 쓰기 전까지 쌓아 둘 수 있는 출력 바이트 수(2 MiB).
///
/// 링 크기(1 MiB)의 두 배다 — 다시 맞추려고 링 전체를 보낸 직후에도 실시간 출력을 담을 여유가 남아야
/// 느린 클라이언트가 재생을 받는 동안 곧바로 또 상한을 넘지 않는다.
pub const MAX_PENDING_BYTES: usize = 2 << 20;

/// 쓰기 스레드가 쌓인 프레임을 모두 썼을 때 불리는 훅.
type DrainedHook = Arc<dyn Fn() + Send + Sync>;

struct Shared {
    /// 보냈지만 아직 소켓에 쓰이지 않은 프레임 내용 바이트 수.
    n_pending_bytes: AtomicUsize,
    /// 상한 때문에 출력을 버렸고 아직 다시 맞추지 않았는가.
    overflowed: AtomicBool,
    on_drained: Mutex<Option<DrainedHook>>,
}

/// 보내는 쪽. 복제해도 같은 통로를 가리킨다.
#[derive(Clone)]
pub struct Outbox {
    tx: Sender<Frame>,
    shared: Arc<Shared>,
}

/// 받는 쪽. 연결의 쓰기 스레드가 쥐고 소켓에 쓴다.
pub struct Inbox {
    rx: Receiver<Frame>,
    shared: Arc<Shared>,
}

/// 새 보내기 통로.
pub fn channel() -> (Outbox, Inbox) {
    let (tx, rx) = mpsc_channel();
    let shared = Arc::new(Shared {
        n_pending_bytes: AtomicUsize::new(0),
        overflowed: AtomicBool::new(false),
        on_drained: Mutex::new(None),
    });
    (Outbox { tx, shared: shared.clone() }, Inbox { rx, shared })
}

impl Outbox {
    /// 프레임을 무조건 보낸다. 응답·알림·재생처럼 버리면 안 되는 프레임에 쓴다.
    ///
    /// 보낸 바이트는 상한 계산에 들어가지만 이 호출은 상한을 넘어도 거절하지 않는다. 연결이 이미 끊겼으면 조용히 버린다.
    pub fn send(
        &self,
        frame: Frame,
    ) {
        self.shared.n_pending_bytes.fetch_add(frame.payload.len(), Ordering::SeqCst);
        let _ = self.tx.send(frame);
    }

    /// 쌓인 바이트가 상한 안일 때만 출력 프레임을 보낸다. 상한을 넘기면 버리고 "다시 맞춰야 함"을 표시하고 `false`다.
    ///
    /// 이미 표시된 연결은 상한과 무관하게 계속 버린다 — 중간 출력만 빠진 화면이 이어 붙지 않게 하기 위해서다.
    pub fn send_output(
        &self,
        frame: Frame,
    ) -> bool {
        let n_pending = self.shared.n_pending_bytes.load(Ordering::SeqCst);
        let fits =
            !self.shared.overflowed.load(Ordering::SeqCst) && n_pending + frame.payload.len() <= MAX_PENDING_BYTES;
        if fits {
            self.send(frame);
        } else {
            self.shared.overflowed.store(true, Ordering::SeqCst);
        }
        fits
    }

    /// 쌓인 프레임이 하나도 없는가. 클라이언트가 쓰인 만큼 모두 읽었다는 뜻이다.
    pub fn is_drained(&self) -> bool {
        self.shared.n_pending_bytes.load(Ordering::SeqCst) == 0
    }

    /// "다시 맞춰야 함" 표시를 지우고 표시돼 있었는지 돌려준다. 표시를 지운 쪽이 재생을 보낼 책임을 진다.
    pub fn take_overflow(&self) -> bool {
        self.shared.overflowed.swap(false, Ordering::SeqCst)
    }

    /// 쓰기 스레드가 쌓인 프레임을 모두 쓰고도 "다시 맞춰야 함" 표시가 남아 있으면 불릴 훅을 등록한다. 이전 훅은 대체된다.
    pub fn set_drained_hook(
        &self,
        hook: impl Fn() + Send + Sync + 'static,
    ) {
        *self.shared.on_drained.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(hook));
    }
}

impl Inbox {
    /// 프레임이 오는 대로 `write`로 쓴다. `write`가 실패하거나 보내는 쪽이 모두 사라지면 돌아온다.
    ///
    /// 쓴 프레임만큼 쌓인 바이트 수를 줄이고, 줄여서 0이 됐을 때 "다시 맞춰야 함" 표시가 있으면 훅을 부른다.
    pub fn run(
        self,
        mut write: impl FnMut(&Frame) -> io::Result<()>,
    ) {
        while let Ok(frame) = self.rx.recv() {
            let written = write(&frame);
            let n_left =
                self.shared.n_pending_bytes.fetch_sub(frame.payload.len(), Ordering::SeqCst) - frame.payload.len();
            if written.is_err() {
                break;
            }
            if n_left == 0 && self.shared.overflowed.load(Ordering::SeqCst) {
                // 훅이 터미널 잠금을 잡으므로, 훅 등록 잠금은 훅을 꺼낸 뒤 풀고 부른다(터미널 잠금 → 훅 등록 잠금 순서와 맞물리지 않게).
                let hook = self.shared.on_drained.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
                if let Some(hook) = hook {
                    hook();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_protocol::message::kind;

    fn output(n_bytes: usize) -> Frame {
        Frame::new(kind::OUTPUT, vec![b'x'; n_bytes])
    }

    #[test]
    fn test_pending_bytes_stop_at_the_limit_and_mark_overflow() {
        let (outbox, _inbox) = channel();
        let chunk = 64 * 1024;
        let mut n_sent = 0;
        for _ in 0..(MAX_PENDING_BYTES * 8 / chunk) {
            if outbox.send_output(output(chunk)) {
                n_sent += chunk;
            }
        }
        assert_eq!(n_sent, MAX_PENDING_BYTES, "nothing past the limit may be queued");
        assert!(!outbox.is_drained());
        // 한 번 넘치면 작은 프레임도 계속 버려 화면이 중간에 끊긴 채 이어 붙지 않게 한다.
        assert!(!outbox.send_output(output(1)));
        assert!(outbox.take_overflow());
        assert!(!outbox.take_overflow(), "taking the mark clears it");
    }

    #[test]
    fn test_control_frames_are_never_refused() {
        let (outbox, inbox) = channel();
        assert!(outbox.send_output(output(MAX_PENDING_BYTES)));
        outbox.send(Frame::text(kind::EXITED, "0"));
        drop(outbox);
        let mut kinds = Vec::new();
        inbox.run(|frame| {
            kinds.push(frame.kind);
            Ok(())
        });
        assert_eq!(kinds, vec![kind::OUTPUT, kind::EXITED]);
    }

    #[test]
    fn test_hook_runs_once_the_queue_drains_after_an_overflow() {
        let (outbox, inbox) = channel();
        assert!(outbox.send_output(output(MAX_PENDING_BYTES)));
        assert!(!outbox.send_output(output(10)));
        let n_hooks = Arc::new(AtomicUsize::new(0));
        let counted = n_hooks.clone();
        outbox.set_drained_hook(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        drop(outbox);
        inbox.run(|_| Ok(()));
        assert_eq!(n_hooks.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_hook_does_not_run_without_an_overflow() {
        let (outbox, inbox) = channel();
        outbox.set_drained_hook(|| panic!("no overflow happened"));
        assert!(outbox.send_output(output(100)));
        drop(outbox);
        inbox.run(|_| Ok(()));
    }

    #[test]
    fn test_failed_write_ends_the_writer() {
        let (outbox, inbox) = channel();
        outbox.send(output(1));
        outbox.send(output(1));
        let mut n_writes = 0;
        inbox.run(|_| {
            n_writes += 1;
            Err(io::Error::other("closed"))
        });
        assert_eq!(n_writes, 1);
    }
}
