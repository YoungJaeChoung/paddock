//! 앱↔데몬 프레임: [종류 u8][길이 u32 LE][내용].
use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// 연결 위로 오가는 메시지 하나. 종류 번호의 뜻은 `message::kind`가 정한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    /// 내용 길이 상한. 이보다 긴 프레임은 보내지도 받지도 않는다.
    pub const MAX_PAYLOAD: usize = 8 << 20;
    const HEADER_LEN: usize = 5;

    /// 종류와 내용으로 프레임을 만든다.
    pub fn new(
        kind: u8,
        payload: Vec<u8>,
    ) -> Self {
        Self { kind, payload }
    }

    /// 값을 JSON 내용으로 담은 프레임.
    pub fn json<T: Serialize>(
        kind: u8,
        value: &T,
    ) -> Self {
        Self::new(kind, serde_json::to_vec(value).expect("message types serialize to JSON"))
    }

    /// 글자 내용(오류 문장·종료 코드)을 담은 프레임.
    pub fn text(
        kind: u8,
        text: &str,
    ) -> Self {
        Self::new(kind, text.as_bytes().to_vec())
    }

    /// 내용을 JSON으로 읽는다. 모양이 맞지 않으면 `InvalidData`다.
    pub fn parse_json<T: DeserializeOwned>(&self) -> io::Result<T> {
        serde_json::from_slice(&self.payload).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// 내용을 글자로 읽는다. 깨진 UTF-8은 대체 문자로 바뀐다.
    pub fn as_text(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }

    /// 프레임을 쓰고 비운다. 내용이 상한보다 길면 아무것도 쓰지 않고 `InvalidInput`이다.
    pub fn write_to(
        &self,
        writer: &mut impl Write,
    ) -> io::Result<()> {
        if self.payload.len() > Self::MAX_PAYLOAD {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))
        } else {
            let mut header = [0u8; Self::HEADER_LEN];
            header[0] = self.kind;
            header[1..].copy_from_slice(&(self.payload.len() as u32).to_le_bytes());
            writer.write_all(&header).and_then(|()| writer.write_all(&self.payload)).and_then(|()| writer.flush())
        }
    }

    /// 프레임 하나를 읽는다. 길이가 상한을 넘으면 내용을 읽기 전에 `InvalidData`이고, 중간에 끊기면 `UnexpectedEof`다.
    pub fn read_from(reader: &mut impl Read) -> io::Result<Self> {
        let mut header = [0u8; Self::HEADER_LEN];
        reader.read_exact(&mut header)?;
        // 개수가 아니라 바이트 길이라 `n_` 접두를 붙이지 않는다.
        let payload_len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if payload_len > Self::MAX_PAYLOAD {
            Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"))
        } else {
            let mut payload = vec![0u8; payload_len];
            reader.read_exact(&mut payload).map(|()| Self::new(header[0], payload))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_c_t1_frame_roundtrip() {
        let mut wire = Vec::new();
        Frame::new(4, b"hello \xed\x95\x9c".to_vec()).write_to(&mut wire).unwrap();
        Frame::new(106, Vec::new()).write_to(&mut wire).unwrap();
        // 머리는 [종류][길이 u32 LE]다.
        assert_eq!(&wire[..5], &[4, 9, 0, 0, 0]);
        let mut reader = Cursor::new(wire);
        let first = Frame::read_from(&mut reader).unwrap();
        assert_eq!((first.kind, first.payload.as_slice()), (4, b"hello \xed\x95\x9c".as_slice()));
        let second = Frame::read_from(&mut reader).unwrap();
        assert_eq!((second.kind, second.payload.len()), (106, 0));
    }

    #[test]
    fn test_frame_larger_than_limit_is_rejected_on_read_and_write() {
        let too_big = (Frame::MAX_PAYLOAD as u32 + 1).to_le_bytes();
        let header = [1, too_big[0], too_big[1], too_big[2], too_big[3]];
        let error = Frame::read_from(&mut Cursor::new(header.to_vec())).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

        let oversized = Frame::new(4, vec![0u8; Frame::MAX_PAYLOAD + 1]);
        let error = oversized.write_to(&mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn test_truncated_frame_is_an_error() {
        let mut wire = Vec::new();
        Frame::new(4, vec![1, 2, 3]).write_to(&mut wire).unwrap();
        wire.truncate(wire.len() - 1);
        let error = Frame::read_from(&mut Cursor::new(wire)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn test_json_and_text_payloads() {
        let frame = Frame::json(2, &serde_json::json!({"a": 1}));
        let value: serde_json::Value = frame.parse_json().unwrap();
        assert_eq!(value["a"], 1);
        assert_eq!(Frame::text(102, "spawn failed").as_text(), "spawn failed");
        let error = Frame::new(2, b"not json".to_vec()).parse_json::<serde_json::Value>().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
