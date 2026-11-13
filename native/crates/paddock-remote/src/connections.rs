//! 열려 있는 기기 WebSocket 목록. 기기를 폐기하면 그 기기의 연결을 바로 끊고, 기기당 동시 연결 수를 제한한다.
use std::collections::HashMap;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 동시에 열 수 있는 WebSocket 수의 한도.
pub struct ConnectionLimit;

impl ConnectionLimit {
    pub const PER_DEVICE: usize = 4;
    /// 모든 기기를 합쳐 동시에 열 수 있는 WebSocket 수. 인증 전 접속의 한도(`http::Limits`)와 따로 센다.
    pub const TOTAL: usize = 32;
}

/// 연결 번호 → (기기 번호, 끊을 때 쓸 소켓 복사본).
type Registry = HashMap<u64, (String, TcpStream)>;

/// 열려 있는 WebSocket의 목록.
#[derive(Default)]
pub struct Connections {
    registry: Arc<Mutex<Registry>>,
    next_id: AtomicU64,
}

/// 목록에 올라 있는 연결 하나. 버리면 목록에서 빠진다.
pub struct ConnectionGuard {
    registry: Arc<Mutex<Registry>>,
    id: u64,
}

impl Connections {
    /// 연결을 목록에 올린다. 그 기기의 연결이 이미 `PER_DEVICE`개이거나 전체가 `TOTAL`개면 `None`이다.
    pub fn register(
        &self,
        device_id: &str,
        stream: &TcpStream,
    ) -> Option<ConnectionGuard> {
        let copy = stream.try_clone().ok()?;
        let mut registry = self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let n_open = registry.values().filter(|(id, _)| id == device_id).count();
        if n_open >= ConnectionLimit::PER_DEVICE || registry.len() >= ConnectionLimit::TOTAL {
            None
        } else {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            registry.insert(id, (device_id.to_string(), copy));
            Some(ConnectionGuard { registry: self.registry.clone(), id })
        }
    }

    /// 기기의 열린 연결을 모두 끊는다. 끊은 연결 수를 돌려준다.
    pub fn close_device(
        &self,
        device_id: &str,
    ) -> usize {
        let registry = self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut n_closed = 0;
        for (id, stream) in registry.values() {
            if id == device_id {
                let _ = stream.shutdown(Shutdown::Both);
                n_closed += 1;
            }
        }
        n_closed
    }

    /// 지금 열려 있는 연결 수.
    pub fn count(&self) -> usize {
        self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).len()
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    /// 루프백으로 이어진 소켓 한 쌍(서버 쪽, 클라이언트 쪽).
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (server, client)
    }

    #[test]
    fn test_per_device_limit_and_guard_drop() {
        let connections = Connections::default();
        let mut keep = Vec::new();
        let mut guards = Vec::new();
        for _ in 0..ConnectionLimit::PER_DEVICE {
            let (server, client) = pair();
            guards.push(connections.register("d1", &server).expect("under the limit"));
            keep.push((server, client));
        }
        let (server, client) = pair();
        assert!(connections.register("d1", &server).is_none(), "limit reached");
        assert!(connections.register("d2", &server).is_some(), "other device is separate");
        drop(client);
        assert_eq!(connections.count(), ConnectionLimit::PER_DEVICE + 1 - 1);
        guards.pop();
        assert!(connections.register("d1", &server).is_some(), "a slot is free after a guard drops");
    }

    #[test]
    fn test_close_device_ends_only_that_devices_connections() {
        let connections = Connections::default();
        let (server_a, mut client_a) = pair();
        let (server_b, mut client_b) = pair();
        let _guard_a = connections.register("a", &server_a).unwrap();
        let _guard_b = connections.register("b", &server_b).unwrap();
        assert_eq!(connections.close_device("a"), 1);
        let mut byte = [0u8; 1];
        assert_eq!(client_a.read(&mut byte).unwrap(), 0, "a's peer sees the end of the stream");
        client_b.set_read_timeout(Some(std::time::Duration::from_millis(100))).unwrap();
        assert!(client_b.read(&mut byte).is_err(), "b stays open (read times out instead of ending)");
    }
}
