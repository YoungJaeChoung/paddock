// backend.log가 커졌을 때 시작할 때 한 번 옛 기록으로 넘기는 회전.
// 백엔드는 터미널을 살리려고 앱 종료 뒤에도 남아 같은 파일에 계속 이어 쓴다. 상한이 없으면 오류가 반복되는 날 수백 MB로 커져
// 파일을 열기도, 원인을 찾기도 어렵다.
const fs = require('node:fs');

/** 로그 회전 규칙. 20 MB를 넘으면 `.1`로 넘기고 `.3`까지만 보관한다(압축하지 않는다 — 열어 읽는 편의가 압축 이득보다 크다). */
class ROTATION {
    static MAX_BYTES = 20 * 1024 * 1024;
    static KEEP = 3;
}

/**
 * 파일이 `maxBytes`를 넘으면 `<path>.1`로 옮기고 기존 `.1`…은 한 칸씩 뒤로 민다. 옮겼으면 true다.
 *
 * 가장 오래된 `.keep`만 지우고 나머지는 보관한다.
 * 파일이 없거나 `maxBytes` 이하이면 아무것도 하지 않는다. 한 번 크게 불어난 파일(수백 MB)도 지우지 않고 `.1`로 넘긴다.
 * 백엔드가 이 파일을 열고 있지 않을 때(새로 띄우기 직전)만 불러야 한다. Windows는 열린 파일의 이름을 바꾸지 못한다.
 *
 * Examples
 * --------
 * | 시작 전                                        | 시작 후                                     |
 * |------------------------------------------------|---------------------------------------------|
 * | `log` 21 MB                                    | `log` 없음, `log.1` 21 MB                    |
 * | `log` 21 MB, `log.1`, `log.2`, `log.3`         | `log.1`=새로 넘긴 것, `log.2`=옛 `.1`, `log.3`=옛 `.2` (옛 `.3` 삭제) |
 * | `log` 1 MB                                     | 그대로                                      |
 */
function rotateIfLarge(
    filePath,
    maxBytes = ROTATION.MAX_BYTES,
    keep = ROTATION.KEEP,
) {
    let isRotated = false;
    const size = fs.existsSync(filePath) ? fs.statSync(filePath).size : 0;
    if (size > maxBytes) {
        fs.rmSync(`${filePath}.${keep}`, { force: true });
        for (let n = keep - 1; n >= 1; n -= 1) {
            if (fs.existsSync(`${filePath}.${n}`)) fs.renameSync(`${filePath}.${n}`, `${filePath}.${n + 1}`);
        }
        fs.renameSync(filePath, `${filePath}.1`);
        isRotated = true;
    }
    return isRotated;
}

/**
 * `rotateIfLarge`를 부르되 실패해도 던지지 않는다. 실패하면 `onFailure(error)`를 부르고 false를 돌려준다.
 *
 * 회전은 백엔드를 띄우기 전의 부수 작업이라 실패(다른 프로그램이 파일을 쥐고 있음 등)가 시작을 막으면 안 된다.
 * 실패해도 원래 파일은 지워지지 않고 그대로 이어 쓴다.
 */
function tryRotate(
    filePath,
    onFailure,
    maxBytes = ROTATION.MAX_BYTES,
    keep = ROTATION.KEEP,
) {
    let isRotated = false;
    try {
        isRotated = rotateIfLarge(filePath, maxBytes, keep);
    } catch (error) {
        onFailure(error);
    }
    return isRotated;
}

module.exports = { ROTATION, rotateIfLarge, tryRotate };
