//! Windows 실행 파일에 앱 아이콘과 버전 정보(VERSIONINFO)를 넣는다.
//!
//! 아이콘은 리소스 번호 1에 둔다. GPUI는 실행 파일의 1번 아이콘을 창과 작업 표시줄 아이콘으로 읽고,
//! 설치 파일의 바로 가기·제거 목록도 이 실행 파일의 첫 아이콘을 쓴다.
//! 버전 정보는 Cargo 패키지 버전(작업 공간 버전 하나)에서 가져와, 탐색기의 파일 속성과 설치 파일의 버전이 같은 값을 보인다.
//! Windows가 아닌 대상(Linux 시험 빌드 등)에서는 아무것도 하지 않는다.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=resources/app.rc");
    println!("cargo:rerun-if-changed=resources/paddock-native.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
        embed_resource::compile("resources/app.rc", version_macros(&version))
            .manifest_optional()
            .expect("the Windows resource file could not be compiled");
    }
}

/// 리소스 파일에 넘길 버전 매크로 두 개: 숫자 네 자리(`FILEVERSION`용)와 표시 문자열.
///
/// 리소스 파일의 `FILEVERSION`은 쉼표로 나눈 숫자 네 개만 받고, 문자열 값은 따옴표로 감싼 리터럴만 받는다.
/// 그래서 같은 버전을 두 모양으로 만든다. 숫자 쪽은 `-beta.1` 같은 꼬리를 빼고, 숫자가 아닌 자리는 0으로 본다.
///
/// Examples
/// --------
/// | version        | 결과                                                                    |
/// |----------------|-------------------------------------------------------------------------|
/// | `0.1.0`        | `PADDOCK_VERSION_NUMBERS=0,1,0,0`, `PADDOCK_VERSION_TEXT="0.1.0"`        |
/// | `1.2.3-beta.1` | `PADDOCK_VERSION_NUMBERS=1,2,3,0`, `PADDOCK_VERSION_TEXT="1.2.3-beta.1"` |
fn version_macros(
    version: &str,
) -> [String; 2] {
    let release = version.split(['-', '+']).next().unwrap_or(version);
    let mut numbers: Vec<String> = release
        .split('.')
        .map(|part| part.parse::<u16>().map(|number| number.to_string()).unwrap_or_else(|_| "0".to_string()))
        .take(4)
        .collect();
    numbers.resize(4, "0".to_string());
    [format!("PADDOCK_VERSION_NUMBERS={}", numbers.join(",")), format!("PADDOCK_VERSION_TEXT=\"{version}\"")]
}
