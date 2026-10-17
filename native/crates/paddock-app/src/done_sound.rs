//! 에이전트 완료 소리: 파일 없이 두 음(880 Hz → 1320 Hz)을 합성해 재생한다.
//!
//! 출력 장치는 재생할 때만 열고 끝나면 닫는다. 상주하는 오디오 스레드·버퍼를 두지 않으려는 것이다.
//! 자동 점검(`automation` 기능)은 사용자의 PC에서 소리가 울리지 않도록 소리를 내는 대신 요청을 사건으로 기록한다.
use std::time::Duration;

use rodio::Source;
use rodio::source::SineWave;

/// 두 음의 모양. 기존 앱과 같다: 둘째 음이 첫 음이 끝나기 전(0.12초 뒤)에 겹쳐 시작한다.
mod tone {
    use std::time::Duration;

    /// (주파수 Hz, 시작 지연).
    pub const NOTES: [(f32, Duration); 2] = [(880.0, Duration::ZERO), (1320.0, Duration::from_millis(120))];
    pub const LENGTH: Duration = Duration::from_millis(250);
    /// 시작할 때 갑자기 켜지며 나는 딱 소리를 막는 짧은 올림.
    pub const FADE_IN: Duration = Duration::from_millis(10);
    /// 끝으로 갈수록 줄어드는 구간. 음이 뚝 끊기지 않게 한다.
    pub const FADE_OUT: Duration = Duration::from_millis(240);
}

/// 소리 크기. 기존 앱과 같은 이득이다.
const GAIN: f32 = 0.18;

/// 완료 소리를 요청한다. `id`는 끝난 터미널의 번호다.
///
/// 일반 실행은 소리를 다른 스레드에서 재생해 화면 갱신을 막지 않는다. 자동 점검 실행은 소리 없이 `sound-requested <id>` 사건만 남긴다.
pub fn request(id: u32) {
    #[cfg(feature = "automation")]
    crate::automation::sound_requested(id);
    #[cfg(not(feature = "automation"))]
    {
        let _ = id;
        std::thread::spawn(play);
    }
}

/// 두 음을 재생하고 끝날 때까지 기다린 뒤 장치를 닫는다. 장치를 열 수 없으면 아무것도 하지 않는다 — 미확인 표시는 소리 없이도 남는다.
pub fn play() {
    if let Ok(device) = rodio::DeviceSinkBuilder::open_default_sink() {
        let mut total = Duration::ZERO;
        for (frequency, delay) in tone::NOTES {
            let note = SineWave::new(frequency)
                .take_duration(tone::LENGTH)
                .amplify(GAIN)
                .fade_in(tone::FADE_IN)
                .fade_out(tone::FADE_OUT)
                .delay(delay);
            device.mixer().add(note);
            total = total.max(delay + tone::LENGTH);
        }
        // 재생이 끝나기 전에 장치를 닫으면 소리가 잘린다.
        std::thread::sleep(total);
    }
}
