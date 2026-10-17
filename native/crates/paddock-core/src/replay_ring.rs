//! 터미널당 원시 출력 링 버퍼. 재접속 때 처음부터 다시 보낼 바이트를 담는다.

/// 고정 용량 링 버퍼. 가득 차면 가장 오래된 바이트부터 덮는다.
pub struct ReplayRing {
    buf: Vec<u8>,
    // 다음에 쓸 위치.
    head: usize,
    // 담긴 바이트 수(용량 이하).
    len: usize,
}

impl ReplayRing {
    /// 비어 있는 링. `capacity`는 0보다 커야 한다.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "ring capacity must be positive");
        Self { buf: vec![0u8; capacity], head: 0, len: 0 }
    }

    /// 바이트를 링 끝에 덧붙인다. 가득 차면 가장 오래된 바이트부터 덮는다.
    pub fn push(
        &mut self,
        data: &[u8],
    ) {
        let cap = self.buf.len();
        // 쓰기가 용량보다 크면 마지막 용량만큼만 남는다.
        let data = if data.len() > cap { &data[data.len() - cap..] } else { data };
        let first = (cap - self.head).min(data.len());
        self.buf[self.head..self.head + first].copy_from_slice(&data[..first]);
        let rest = data.len() - first;
        if rest > 0 {
            self.buf[..rest].copy_from_slice(&data[first..]);
        }
        self.head = (self.head + data.len()) % cap;
        self.len = (self.len + data.len()).min(cap);
    }

    /// 오래된 순으로 복사한 내용. 링이 한 바퀴 돌아 시작이 글자 중간이면 UTF-8 이어지는 바이트를 버려 글자 경계에서 시작한다.
    ///
    /// Examples
    /// --------
    /// | 담긴 상태                         | 결과                              |
    /// |-----------------------------------|-----------------------------------|
    /// | 용량 미만                         | 쓴 순서 그대로                    |
    /// | 가득 참, 시작이 한글 둘째 바이트  | 그 글자를 통째로 버리고 다음 글자부터 |
    pub fn snapshot(&self) -> Vec<u8> {
        let cap = self.buf.len();
        let start = (self.head + cap - self.len) % cap;
        let mut out = Vec::with_capacity(self.len);
        if start + self.len <= cap {
            out.extend_from_slice(&self.buf[start..start + self.len]);
        } else {
            out.extend_from_slice(&self.buf[start..]);
            out.extend_from_slice(&self.buf[..self.head]);
        }
        let wrapped = self.len == cap;
        let mut skip = 0;
        if wrapped {
            while skip < out.len() && (out[skip] & 0xc0) == 0x80 {
                skip += 1;
            }
        }
        out.drain(..skip);
        out
    }

    /// 가장 최근에 담긴 바이트를 최대 `max_bytes`개 오래된 순으로 복사한다. 링 전체(`snapshot`)를 복사하지 않아도 되는 화면 꼬리용이다.
    ///
    /// 담긴 바이트가 `max_bytes`개 이하이면 `snapshot`과 같다. 더 많으면 뒤쪽 `max_bytes`개를 돌려주며 시작이 글자 중간일 수 있으므로
    /// 호출자가 글자 경계를 맞춘다(`terminal_history::output_tail`).
    ///
    /// Examples
    /// --------
    /// | 담긴 내용     | `max_bytes` | 결과    |
    /// |---------------|-------------|---------|
    /// | `abcde`       | 3           | `cde`   |
    /// | `abcde`       | 9           | `abcde` |
    pub fn last_bytes(
        &self,
        max_bytes: usize,
    ) -> Vec<u8> {
        if max_bytes >= self.len {
            self.snapshot()
        } else {
            let cap = self.buf.len();
            let start = (self.head + cap - max_bytes) % cap;
            let mut out = Vec::with_capacity(max_bytes);
            if start + max_bytes <= cap {
                out.extend_from_slice(&self.buf[start..start + max_bytes]);
            } else {
                out.extend_from_slice(&self.buf[start..]);
                out.extend_from_slice(&self.buf[..self.head]);
            }
            out
        }
    }

    /// 지금 담긴 바이트 수(용량 이하).
    pub fn len(&self) -> usize {
        self.len
    }

    /// 담긴 바이트가 없는가.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_d3a_ring_keeps_order() {
        let mut ring = ReplayRing::new(8);
        ring.push(b"abc");
        ring.push(b"de");
        assert_eq!(ring.snapshot(), b"abcde");
        assert_eq!(ring.len(), 5);
    }

    #[test]
    fn test_c_d11c_last_bytes_returns_the_newest_bytes_across_the_wrap_point() {
        let mut ring = ReplayRing::new(8);
        ring.push(b"abcde");
        assert_eq!(ring.last_bytes(3), b"cde");
        assert_eq!(ring.last_bytes(9), b"abcde");
        ring.push(b"fghij");
        assert_eq!(ring.last_bytes(6), b"efghij");
        assert_eq!(ring.last_bytes(8), b"cdefghij");
    }

    #[test]
    fn test_c_d3a_ring_wrapped_starts_at_char_boundary() {
        // "가나"는 6바이트이고 용량은 5바이트라, 그대로 자르면 "가"의 둘째 바이트에서 시작한다. 그래서 "가" 전체를 버린다.
        let mut ring = ReplayRing::new(5);
        ring.push("가나".as_bytes());
        assert_eq!(ring.snapshot(), "나".as_bytes());
    }

    #[test]
    fn test_c_d3a_ring_oversized_write_keeps_tail() {
        let mut ring = ReplayRing::new(4);
        ring.push(b"0123456789");
        assert_eq!(ring.snapshot(), b"6789");
    }

    #[test]
    fn test_c_d3a_ring_1_5_mb_of_korean_snapshot_is_clean() {
        const MIB: usize = 1 << 20;
        let mut ring = ReplayRing::new(MIB);
        let text = "한글 출력 줄 ";
        let mut written = String::new();
        while written.len() < MIB + MIB / 2 {
            written.push_str(text);
        }
        // 한 바퀴 도는 지점이 글자 중간에 오도록 고르지 않은 크기로 나눠 넣는다.
        for chunk in written.as_bytes().chunks(4093) {
            ring.push(chunk);
        }
        let snapshot = ring.snapshot();
        assert!(snapshot.len() <= MIB);
        // 올바른 UTF-8이면 스냅샷이 글자 경계에서 시작하고 끝에서 잘린 글자도 없다.
        let text_out = std::str::from_utf8(&snapshot).expect("snapshot must be valid UTF-8");
        // 스냅샷은 쓴 내용의 뒷부분이다. 중복이나 순서 뒤바뀜이 없다.
        assert!(written.ends_with(text_out));
    }
}
