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

module.exports = { validateEntryName };
