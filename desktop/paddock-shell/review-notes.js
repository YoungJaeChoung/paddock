/**
 * 코드를 보다 줄에 남긴 검토 메모를 모아 에이전트에게 한 번에 보낼 글로 바꾸는 순수 계산.
 *
 * 에이전트에게 "이 파일 이 줄을 이렇게 고쳐"를 말로 풀어 쓰는 대신, 메모마다 파일·줄·그 줄의 내용을 붙여 한 덩어리로 넘기려고 존재한다.
 * 줄 내용을 함께 보내는 이유는 에이전트가 파일을 고친 뒤에도 메모가 가리키는 줄을 알아볼 수 있게 하기 위해서다.
 * 메모는 앱 화면(창) 안에서만 모으고 파일로 남기지 않는다. 보내면 비운다.
 */

const NOTES = {
    // 한 줄 내용을 글에 담을 때 자르는 길이. 긴 줄(번들·데이터)이 글 전체를 키우지 않게 한다.
    MAX_LINE_CHARS: 160,
    // 한 번에 보낼 수 있는 메모 수. 넘으면 보내기 전에 나눠 보내도록 알린다.
    MAX_NOTES: 50,
};

/**
 * 파일 경로를 작업 폴더 기준 상대 경로로 바꾼다. 어느 작업 폴더에도 속하지 않으면 원래 경로 그대로다.
 * 폴더 경계를 지켜 `/repo`가 `/repo-other/a.js`에 맞지 않게 한다.
 *
 * Examples:
 *   ('/repo/src/a.js', ['/repo'])        → 'src/a.js'
 *   ('/repo-other/a.js', ['/repo'])      → '/repo-other/a.js'
 *   ('/x/a.js', [])                      → '/x/a.js'
 */
function relativePath(
    filePath,
    roots,
) {
    const root = roots.find(candidate => filePath.startsWith(candidate.endsWith('/') ? candidate : `${candidate}/`));
    const trimmed = root ? root.replace(/\/$/, '') : null;
    return trimmed === null ? filePath : filePath.slice(trimmed.length + 1);
}

/** 줄 내용에서 앞뒤 공백을 걷고 길이를 줄인다. */
function shorten(
    text,
) {
    const trimmed = text.trim();
    return trimmed.length > NOTES.MAX_LINE_CHARS ? `${trimmed.slice(0, NOTES.MAX_LINE_CHARS)}…` : trimmed;
}

/**
 * 메모 목록을 에이전트에게 붙여 넣을 글로 만든다. 메모가 없으면 빈 문자열이다.
 * 줄 번호는 1부터 센다. 여러 줄을 고른 메모는 `시작-끝`으로 쓰고, 첫 줄 내용만 붙인다.
 *
 * Examples:
 *   [{ file: 'src/a.js', startLine: 3, endLine: 3, lineText: '  return x;', text: '널 검사 필요' }]
 *     →  'Review notes (1):\n\n1. src/a.js:3\n   > return x;\n   널 검사 필요\n'
 *   [{ file: 'b.js', startLine: 5, endLine: 9, lineText: 'if (a) {', text: '분리' }]
 *     →  '… 1. b.js:5-9\n   > if (a) {\n   분리\n'
 */
function formatNotes(
    notes,
) {
    let text = '';
    if (notes.length) {
        const blocks = notes.map((note, index) => {
            const range = note.endLine > note.startLine ? `${note.startLine}-${note.endLine}` : `${note.startLine}`;
            const indented = note.text.trim().split('\n').join('\n   ');
            return `${index + 1}. ${note.file}:${range}\n   > ${shorten(note.lineText)}\n   ${indented}`;
        });
        text = `Review notes (${notes.length}):\n\n${blocks.join('\n\n')}\n`;
    }
    return text;
}

module.exports = {
    NOTES,
    relativePath,
    formatNotes,
};
