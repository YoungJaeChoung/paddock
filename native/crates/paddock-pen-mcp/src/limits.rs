//! 기다리는 시간과 횟수의 상한. 도구마다 흩어지지 않게 한 곳에 둔다.
use std::time::Duration;

/// Pen 앱과 주고받는 일의 시간·횟수 기준.
pub struct Limits;

impl Limits {
    /// Pencil 도구를 한 번 부를 때 답을 기다리는 시간.
    pub const PENCIL_CALL_DEADLINE: Duration = Duration::from_secs(8);
    /// 열기 뒤 파일이 활성 문서가 되기를 기다리는 시간. 앱은 WSL 파일을 처음 열 때 33초까지 걸린 적이 있다.
    pub const OPEN_DEADLINE: Duration = Duration::from_secs(60);
    /// 활성 문서를 다시 확인하는 간격.
    pub const OPEN_INTERVAL: Duration = Duration::from_secs(3);
    /// 열기 요청 뒤 이만큼 지나도 활성 문서가 바뀌지 않으면 한 번 더 요청한다. 꺼져 있던 앱이 막 뜰 때 받은 첫 요청은
    /// 창만 열고 활성 문서에 반영되지 않는 경우가 있다.
    pub const OPEN_REPEAT_AFTER: Duration = Duration::from_secs(12);
    /// 이 크기(바이트) 이하의 파일은 빈 문서일 수 있어, 앱이 빈 문서로 읽어도 이상으로 보지 않는다.
    pub const EMPTY_DOCUMENT_MAX_BYTES: u64 = 1024;
    /// 앱이 내용 있는 파일을 문서로 읽을 때까지 기다리는 시간.
    pub const LOAD_DEADLINE: Duration = Duration::from_secs(12);
    /// 저장을 부른 뒤 파일 수정 시각이 바뀌기를 기다리는 시간.
    pub const SAVE_DEADLINE: Duration = Duration::from_secs(10);
    /// 파일 수정 시각을 다시 보는 간격.
    pub const SAVE_INTERVAL: Duration = Duration::from_millis(500);
    /// 메뉴가 늦게 열리거나 열기 직후 활성 문서가 되돌아가는 일은 잠깐 뒤 풀리는 경우가 많아 저장 전체를 몇 번 더 시도한다.
    pub const SAVE_ATTEMPTS: usize = 4;
    /// 저장 재시도 사이의 간격.
    pub const SAVE_RETRY_DELAY: Duration = Duration::from_secs(1);
    /// 저장 잠금을 기다리는 시간. 넘으면 열기·저장을 하지 않고 실패로 돌려준다. 잠금을 쥐는 시간은 저장 하나가 보통 15초 안팎(대상을
    /// 활성으로 만들기·메뉴 조작·수정 시각 확인·이전 활성 문서로 되돌리기), 열기 하나가 WSL 파일을 처음 열 때 45초 안팎(활성 문서가 되기까지
    /// 실측 33초 + 문서를 읽기까지 `LOAD_DEADLINE`)이라, 앞에 하나가 줄 서 있어도 이 안에 차례가 온다.
    pub const SAVE_LOCK_DEADLINE: Duration = Duration::from_secs(90);
    /// Pencil 저장 도구를 부를 때 답을 기다리는 시간. 큰 파일(1.4 MB)을 WSL 경로에 쓰면 `PENCIL_CALL_DEADLINE`을 넘길 수 있다.
    pub const PENCIL_SAVE_DEADLINE: Duration = Duration::from_secs(30);
    /// 파일의 Pen 창이 나타나기를 기다리는 시간.
    pub const WINDOW_DEADLINE: Duration = Duration::from_secs(3);
    /// 창·메뉴 팝업을 다시 찾는 간격.
    pub const WINDOW_INTERVAL: Duration = Duration::from_millis(100);
    /// 최소화를 풀고 나서 메뉴를 누르기 전에 쉬는 시간. 최소화된 창은 되살리지 않으면 메뉴가 열리지 않는다(실측 0/6).
    pub const RESTORE_SETTLE: Duration = Duration::from_millis(600);
    /// File 메뉴를 누른 뒤 팝업 창이 뜨기를 기다리는 시간.
    pub const MENU_OPEN_DEADLINE: Duration = Duration::from_secs(2);
    /// 메뉴를 닫으라고 한 뒤 팝업 창이 사라지기를 기다리는 시간.
    pub const MENU_CLOSE_DEADLINE: Duration = Duration::from_secs(1);
}
