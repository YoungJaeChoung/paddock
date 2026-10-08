// 확장 버전 비교. 화면 모듈(extension-open.js)과 분리해 Theia 없이 시험한다.

/**
 * 버전 문자열 둘을 비교해 a가 b보다 새로우면 양수, 같으면 0, 오래되면 음수를 돌려준다.
 *
 * 확장이 "업데이트가 있다"고 알릴 때 설치본보다 새 버전이 실제로 있는지 가르는 데 쓴다.
 * 숫자 부분(major.minor.patch)을 먼저 비교하고, 숫자가 같으면 접미사(`-beta` 등)가 없는 쪽이 새롭다.
 *
 * | a | b | 결과 |
 * |---|---|---|
 * | 0.6.80 | 0.6.74 | 양수 |
 * | 0.6.74 | 0.6.74 | 0 |
 * | 1.0.0 | 1.0.0-beta | 양수 |
 * | 0.10.0 | 0.9.9 | 양수 |
 */
function compareVersions(
    a,
    b,
) {
    const split = version => {
        const [core, suffix = ''] = String(version).split('-', 2);
        return { parts: core.split('.').map(part => Number.parseInt(part, 10) || 0), suffix };
    };
    const left = split(a);
    const right = split(b);
    let result = 0;
    for (let index = 0; index < Math.max(left.parts.length, right.parts.length) && result === 0; index += 1) {
        result = (left.parts[index] ?? 0) - (right.parts[index] ?? 0);
    }
    if (result === 0 && left.suffix !== right.suffix) result = left.suffix === '' ? 1 : right.suffix === '' ? -1 : left.suffix.localeCompare(right.suffix);
    return result;
}

module.exports = { compareVersions };
