// 터미널 글자 위의 파일 경로 링크를 거른다.
// Theia의 파일 경로 판별은 `.`·`..`·`~` 하나만 있어도 경로(현재 폴더·상위 폴더·홈)로 본다. 그래서 문장 끝 마침표마다
// 마우스를 올리면 "Follow link" 상자가 뜨고, Ctrl+클릭하면 파일 찾기 창이 열린다. 이런 글자는 링크로 내지 않는다.

/**
 * 링크 글자가 경로 표시(`.`·`..`·`~`)뿐인지. 뒤에 줄·칸 번호(`:12`)가 붙어도 같다.
 *
 * Examples:
 *   text            → 결과
 *   '.'             → true
 *   '..'            → true
 *   '~'             → true
 *   '.:12'          → true
 *   './src'         → false
 *   '~/notes.md'    → false
 *   'orders.js'     → false
 */
function isBarePathMark(
    text,
) {
    return /^(\.{1,2}|~)(:\d+)*$/.test(text);
}

/**
 * 링크 처리기의 `provideLinks`를 감싸, 줄에서 찾은 링크 중 경로 표시뿐인 것을 뺀다.
 * 처리기는 모든 터미널이 공유하는 하나라 만들어지는 순간(DI 활성화) 한 번 감싼다. 같은 처리기를 다시 넘겨도 두 번 감싸지 않는다.
 * 감싼 처리기를 그대로 돌려준다.
 */
function withoutBarePathLinks(
    provider,
) {
    if (provider && !provider.paddockSkipsBarePathMarks) {
        const provideLinks = provider.provideLinks.bind(provider);
        provider.paddockSkipsBarePathMarks = true;
        provider.provideLinks = async (line, terminal) => {
            const links = await provideLinks(line, terminal);
            return links.filter(link => !isBarePathMark(line.substr(link.startIndex, link.length)));
        };
    }
    return provider;
}

module.exports = { isBarePathMark, withoutBarePathLinks };
