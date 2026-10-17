//! 터미널 출력 바이트에서 OSC 시퀀스(`ESC ] 번호 ; 내용 BEL` 또는 `… ESC \`)를 읽는 스캐너.
//!
//! 데몬이 터미널 출력을 처음 보는 자리에서 셸이 알리는 현재 폴더(OSC 7), 에이전트가 알리는 상태(OSC 7501),
//! 프로그램이 정하는 제목(OSC 0·2)을 읽는 데 쓴다. 출력은 임의 크기 조각으로 오므로 시퀀스가 조각 경계에 걸려도 읽도록
//! 읽는 도중의 상태와 본문을 다음 호출로 넘긴다. 이 모듈은 화면을 그리지 않으며 필요한 OSC 말고는 아무것도 하지 않는다.

/// 본문 한 개가 가질 수 있는 최대 바이트 수. 이보다 긴 시퀀스(큰 클립보드 내용 등)는 끝까지 읽지 않고 버린다.
pub const MAX_BODY_BYTES: usize = 512;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// 읽은 OSC 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OscEvent {
    /// OSC 7: 셸이 알린 현재 폴더(주소이거나 경로).
    Cwd(String),
    /// OSC 7501: 에이전트가 알린 상태 본문(해석은 `agent_model::parse_program_status`가 한다).
    ProgramStatus(String),
    /// OSC 0·2: 터미널 제목.
    Title(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// OSC 밖.
    #[default]
    Ground,
    /// ESC를 읽었다. 다음이 `]`이면 OSC가 시작된다.
    Escape,
    /// OSC 본문을 모으는 중.
    Body,
    /// 본문 중 ESC를 읽었다. 다음이 `\`이면 끝이고 `]`이면 새 OSC다.
    BodyEscape,
    /// 너무 긴 본문을 끝까지 버리는 중.
    Skip,
    SkipEscape,
}

/// OSC 스캐너. 터미널마다 하나씩 쥐고 출력 조각을 순서대로 `feed`한다.
#[derive(Debug, Default)]
pub struct OscScanner {
    state: State,
    body: Vec<u8>,
}

impl OscScanner {
    /// 출력 조각 하나를 읽고, 그 조각에서 끝난 OSC를 순서대로 돌려준다.
    ///
    /// 끝나지 않은 시퀀스는 다음 호출로 이어진다(최대 `MAX_BODY_BYTES`). 본문 중 새 `ESC ]`가 오면 앞 시퀀스는 버리고 새로 시작한다.
    /// 필요한 번호(0·2·7·7501)가 아닌 OSC는 이벤트 없이 지나간다.
    pub fn feed(
        &mut self,
        data: &[u8],
    ) -> Vec<OscEvent> {
        let mut events = Vec::new();
        let mut index = 0;
        while index < data.len() {
            if self.state == State::Ground {
                // 대부분의 출력은 ESC가 없는 글자다. 다음 ESC까지 한 번에 건너뛴다.
                match data[index..].iter().position(|byte| *byte == ESC) {
                    Some(offset) => {
                        index += offset;
                        self.state = State::Escape;
                    }
                    None => index = data.len(),
                }
            } else {
                self.step(data[index], &mut events);
            }
            index += 1;
        }
        events
    }

    fn step(
        &mut self,
        byte: u8,
        events: &mut Vec<OscEvent>,
    ) {
        match self.state {
            State::Ground => {}
            State::Escape => match byte {
                b']' => self.begin(),
                ESC => {}
                _ => self.state = State::Ground,
            },
            State::Body => match byte {
                BEL => self.finish(events),
                ESC => self.state = State::BodyEscape,
                _ if self.body.len() >= MAX_BODY_BYTES => {
                    self.body.clear();
                    self.state = State::Skip;
                }
                _ => self.body.push(byte),
            },
            State::BodyEscape => match byte {
                b'\\' => self.finish(events),
                b']' => self.begin(),
                ESC => {}
                _ => {
                    self.body.clear();
                    self.state = State::Ground;
                }
            },
            State::Skip => match byte {
                BEL => self.state = State::Ground,
                ESC => self.state = State::SkipEscape,
                _ => {}
            },
            State::SkipEscape => match byte {
                b']' => self.begin(),
                ESC => {}
                _ => self.state = State::Ground,
            },
        }
    }

    fn begin(&mut self) {
        self.body.clear();
        self.state = State::Body;
    }

    fn finish(
        &mut self,
        events: &mut Vec<OscEvent>,
    ) {
        let text = String::from_utf8_lossy(&self.body).into_owned();
        self.body.clear();
        self.state = State::Ground;
        let (number, content) = text.split_once(';').unwrap_or((text.as_str(), ""));
        let content = content.to_string();
        match number {
            "0" | "2" => events.push(OscEvent::Title(content)),
            "7" => events.push(OscEvent::Cwd(content)),
            "7501" => events.push(OscEvent::ProgramStatus(content)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(
        scanner: &mut OscScanner,
        chunks: &[&[u8]],
    ) -> Vec<OscEvent> {
        chunks.iter().flat_map(|chunk| scanner.feed(chunk)).collect()
    }

    #[test]
    fn test_osc_scan_reads_cwd_status_and_title_with_either_terminator() {
        let mut scanner = OscScanner::default();
        let input = b"before\x1b]7;file://host/home/me\x07mid\x1b]7501;state=working\x1b\\\x1b]0;my title\x07\x1b]2;other\x1b\\after";
        assert_eq!(
            scanner.feed(input),
            vec![
                OscEvent::Cwd("file://host/home/me".to_string()),
                OscEvent::ProgramStatus("state=working".to_string()),
                OscEvent::Title("my title".to_string()),
                OscEvent::Title("other".to_string()),
            ]
        );
    }

    #[test]
    fn test_osc_scan_finds_a_sequence_split_at_every_byte_boundary() {
        let input = b"x\x1b]7501;?\x1b\\y\x1b]7;C:/Users/me\x07z";
        let expected = vec![OscEvent::ProgramStatus("?".to_string()), OscEvent::Cwd("C:/Users/me".to_string())];
        for split in 0..=input.len() {
            let mut scanner = OscScanner::default();
            let (head, tail) = input.split_at(split);
            assert_eq!(scan_all(&mut scanner, &[head, tail]), expected, "split at {split}");
        }
        let mut scanner = OscScanner::default();
        let bytes: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(scan_all(&mut scanner, &bytes), expected);
    }

    #[test]
    fn test_osc_scan_ignores_other_sequences_and_plain_escapes() {
        let mut scanner = OscScanner::default();
        let input = b"\x1b[31mred\x1b[0m\x1b]8;;https://example.com\x07link\x1b]52;c;AAAA\x07\x1b]133;A\x07";
        assert!(scanner.feed(input).is_empty());
        // 끝나지 않은 다른 OSC 뒤에 와도 다음 OSC는 읽는다.
        assert_eq!(
            scan_all(&mut scanner, &[b"\x1b]8;;", b"https://a\x07\x1b]7;/tmp\x07"]),
            vec![OscEvent::Cwd("/tmp".to_string())]
        );
    }

    #[test]
    fn test_osc_scan_drops_a_body_longer_than_the_carry_limit_and_recovers() {
        let mut scanner = OscScanner::default();
        let mut long = b"\x1b]0;".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_BODY_BYTES + 100));
        long.push(0x07);
        long.extend_from_slice(b"\x1b]7;/ok\x07");
        assert_eq!(scanner.feed(&long), vec![OscEvent::Cwd("/ok".to_string())]);
    }

    #[test]
    fn test_osc_scan_restarts_on_a_new_introducer_inside_an_unfinished_sequence() {
        let mut scanner = OscScanner::default();
        assert_eq!(scanner.feed(b"\x1b]7;/lost\x1b]7;/kept\x07"), vec![OscEvent::Cwd("/kept".to_string())]);
    }
}
