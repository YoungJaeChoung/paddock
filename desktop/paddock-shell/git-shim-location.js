// WSL 폴더용 git 분배기(bin/git.exe)가 어디 있는지 정하는 순수 함수.
const path = require('node:path');

/**
 * 분배기 실행 파일의 경로를 돌려준다. Windows에서 파일이 있을 때만 경로이고, 아니면 null.
 *
 * 왜 있는가: Windows 앱의 Git 보기가 WSL 폴더의 저장소를 Windows git으로 열면, WSL 쪽 git이 커밋하는 순간 파일 크기를 읽지 못해
 * "mmap: could not determine filesize"로 실패한다. 분배기는 현재 폴더가 WSL이면 그 배포판 안의 git을, 아니면 Windows git을 실행한다.
 * 이 함수의 결과가 Git 보기의 `git.path` 기본값이 된다. 분배기를 만들지 못한 설치(null)는 예전처럼 Windows git을 쓴다.
 *
 * Parameters
 * ----------
 * platform : string
 *     `process.platform` 값. 분배기는 Windows 전용이다.
 * projectPath : string
 *     앱 폴더(`paddock-shell/`이 있는 곳).
 * exists : (file: string) => boolean
 *     파일 존재 확인. 테스트에서 바꿔 끼운다.
 *
 * Examples
 * --------
 * | platform | 파일 | 결과                                      |
 * |----------|------|-------------------------------------------|
 * | win32    | 있음 | `<projectPath>\paddock-shell\bin\git.exe` |
 * | win32    | 없음 | null                                      |
 * | linux    | 있음 | null                                      |
 */
function gitShimPath(
    platform,
    projectPath,
    exists,
) {
    let location = null;
    if (platform === 'win32') {
        const candidate = path.win32.join(projectPath, 'paddock-shell', 'bin', 'git.exe');
        location = exists(candidate) ? candidate : null;
    }
    return location;
}

module.exports = { gitShimPath };
