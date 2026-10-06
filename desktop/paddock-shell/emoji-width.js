// 터미널 출력에서 "원래 한 칸인 기호 + 이모지 표시 문자(U+FE0F)"를 두 칸으로 맞추는 순수 함수.
// Claude Code(Bun)·Codex(Rust) 같은 화면 프로그램은 ⚠️·✔️·❤️처럼 이모지 표시 문자가 붙은 기호를 두 칸으로 세고 그 폭에 맞춰 커서를 옮긴다.
// 터미널 그리기 모듈(xterm 5.3)은 문자 하나씩 폭을 정해 이 기호를 한 칸으로 그리므로, 그 줄의 나머지가 한 칸 왼쪽으로 밀리고
// 다시 그릴 때 이전 글자·표 선이 옆 칸에 겹쳐 남는다. 기호 뒤에 빈칸 하나를 채워 화면 프로그램이 센 두 칸을 실제로 차지하게 한다.
// ✅·😀처럼 원래 두 칸인 이모지는 Unicode 11 글자 폭 기준이 두 칸으로 그리므로 건드리지 않는다.

// 기호 + 이모지 표시 문자 + (숫자·#·*이면) 키캡 표시(U+20E3). 숫자·#·*는 키캡까지 붙어야 이모지로 센다.
const TEXT_EMOJI_SEQUENCE = /(\p{Emoji})️(⃣?)/gu;
const WIDE_EMOJI = /\p{Emoji_Presentation}/u;
const SPLIT_SELECTOR = /^️(⃣?)/u;

/** 이모지 표시 문자 앞의 기호가 원래 한 칸이라 빈칸을 채워야 하는지. */
function needsFiller(
    base,
    keycap,
) {
    const isAscii = base.codePointAt(0) < 0x80;
    return !WIDE_EMOJI.test(base) && (!isAscii || keycap === '⃣');
}

/**
 * 터미널 하나의 출력 조각을 차례로 받아, 한 칸 기호 + 이모지 표시 문자 뒤에 빈칸을 넣은 조각을 돌려주는 함수를 만든다.
 *
 * 출력은 조각으로 나뉘어 오므로 기호와 표시 문자가 서로 다른 조각에 걸칠 수 있다. 만든 함수는 직전 조각의 마지막 문자를 기억해
 * 다음 조각이 표시 문자로 시작하면 그 뒤에도 빈칸을 넣는다. 터미널마다 따로 만들어 쓴다.
 *
 * Examples
 * --------
 * | 입력 조각        | 출력 조각         |
 * | ---------------- | ----------------- |
 * | `⚠️전제`         | `⚠️ 전제`         |
 * | `✅완료`         | `✅완료`          |
 * | `1️⃣번`           | `1️⃣ 번`           |
 * | `⚠` 다음 `️전제` | `⚠` 다음 `️ 전제` |
 */
function createEmojiWidthFixer() {
    let previousChar = '';
    return (
        chunk,
    ) => {
        let fixed = chunk.replace(TEXT_EMOJI_SEQUENCE, (match, base, keycap) => (needsFiller(base, keycap) ? `${match} ` : match));
        const split = SPLIT_SELECTOR.exec(fixed);
        if (split && /\p{Emoji}/u.test(previousChar) && needsFiller(previousChar, split[1])) {
            fixed = `${split[0]} ${fixed.slice(split[0].length)}`;
        }
        // 마지막 문자가 두 단위(서로게이트 쌍)일 수 있어 끝의 두 단위만 문자로 나눈다.
        if (chunk) previousChar = Array.from(chunk.slice(-2)).pop();
        return fixed;
    };
}

module.exports = { createEmojiWidthFixer };
