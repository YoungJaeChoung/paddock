// 사이드바에서 새 파일·폴더를 만들 때 입력한 이름을 검사하는 순수 함수. DOM·Theia에 기대지 않아 Node 테스트로 바로 검증한다.

/**
 * 새 파일·폴더 이름의 입력 창 검사 결과.
 *
 * 입력 창은 결과가 빈 문자열이면 확인 버튼을 켜고, 문자열이면 그 문장을 경고로 보이며 버튼을 끈다.
 * 아직 아무것도 입력하지 않은 상태는 잘못이 아니므로 경고 없이 버튼만 끈다(false).
 *
 * Examples
 * --------
 * | 입력          | 결과                                   |
 * | ------------- | -------------------------------------- |
 * | `notes.md`    | `''`                                   |
 * | `` / `  `     | `false`                                |
 * | `a/b`, `a\b`  | 이름에 `/`·`\`를 쓸 수 없다는 경고     |
 * | `.`, `..`     | 예약된 이름이라는 경고                 |
 */
function validateEntryName(
    value,
) {
    let result = '';
    if (!value.trim()) result = false;
    else if (/[\\/]/.test(value)) result = 'A name can’t contain / or \\. Create one level at a time.';
    else if (value === '.' || value === '..') result = '“.” and “..” are reserved. Choose another name.';
    else if (/[\x00-\x1f]/.test(value)) result = 'Remove control characters from the name.';
    return result;
}

/**
 * 같은 이름이 있는 폴더에 붙여 넣거나 끌어 놓은 항목에 붙일 n번째 다른 이름.
 *
 * 덮어쓰지도, 이름을 묻지도 않고 이어 받을 수 있게 한다. 호출자는 0부터 n을 올리며 비어 있는 이름을 찾는다.
 * 점으로 시작하고 다른 점이 없는 이름(`.env`)은 이름 전체를 앞부분으로 본다.
 *
 * Examples
 * --------
 * | name        | n | 결과                |
 * | ----------- | - | ------------------- |
 * | `image.png` | 0 | `image.png`         |
 * | `image.png` | 1 | `image copy.png`    |
 * | `image.png` | 2 | `image copy 2.png`  |
 * | `a.tar.gz`  | 1 | `a.tar copy.gz`     |
 * | `.env`      | 1 | `.env copy`         |
 * | `Makefile`  | 3 | `Makefile copy 3`   |
 */
function copyEntryName(
    name,
    n,
) {
    const dot = name.lastIndexOf('.');
    const hasExtension = dot > 0;
    const stem = hasExtension ? name.slice(0, dot) : name;
    const extension = hasExtension ? name.slice(dot) : '';
    const suffix = n === 0 ? '' : n === 1 ? ' copy' : ` copy ${n}`;
    return `${stem}${suffix}${extension}`;
}

module.exports = { validateEntryName, copyEntryName };
