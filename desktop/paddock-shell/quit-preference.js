// 앱을 닫을 때 남은 터미널을 어떻게 할지 정하는 설정(paddock.backend.onQuit)을 설정 파일(settings.json)에서 읽고 쓴다.
// 설정 파일은 사용자가 손으로 고치는 주석 섞인 JSON이라, 읽을 때는 이 키의 값만 찾고 쓸 때는 나머지 내용·서식을 그대로 둔다.

/** 설정 키. 값은 ASK·KEEP·STOP 중 하나다. */
const KEY = 'paddock.backend.onQuit';

const ON_QUIT = Object.freeze({ ASK: 'ask', KEEP: 'keep', STOP: 'stop' });

const VALUES = Object.values(ON_QUIT);

/** 이 키와 값을 찾는 식. 값이 무엇이든(잘못된 값 포함) 한 줄 문자열 값을 잡는다. */
const ENTRY = new RegExp(`"${KEY.replace(/\./g, '\\.')}"\\s*:\\s*"([^"\\n]*)"`);

/** 문자열 밖의 `//`·`/* *\/` 주석을 지운다. 문자열 안의 `//`(예: URL)는 그대로 둔다. */
function stripComments(
    text,
) {
    let result = '';
    let isString = false;
    for (let index = 0; index < text.length; index += 1) {
        const char = text[index];
        const next = text[index + 1];
        if (isString) {
            result += char;
            if (char === '\\') { result += next || ''; index += 1; }
            else if (char === '"') isString = false;
        } else if (char === '"') {
            isString = true;
            result += char;
        } else if (char === '/' && next === '/') {
            while (index < text.length && text[index] !== '\n') index += 1;
            result += '\n';
        } else if (char === '/' && next === '*') {
            const end = text.indexOf('*/', index + 2);
            index = end === -1 ? text.length : end + 1;
        } else {
            result += char;
        }
    }
    return result;
}

/** 주석·문자열 밖의 첫 `{` 위치. 없으면 -1이다. */
function openingBrace(
    text,
) {
    const stripped = stripComments(text);
    // 주석을 지우면 위치가 밀리므로, 주석을 같은 길이의 공백으로 바꾼 사본에서 찾는다.
    let masked = '';
    let isString = false;
    for (let index = 0; index < text.length; index += 1) {
        const char = text[index];
        const next = text[index + 1];
        if (isString) {
            masked += char;
            if (char === '\\') { masked += next || ''; index += 1; }
            else if (char === '"') isString = false;
        } else if (char === '"') {
            isString = true;
            masked += char;
        } else if (char === '/' && next === '/') {
            while (index < text.length && text[index] !== '\n') { masked += ' '; index += 1; }
            index -= 1;
        } else if (char === '/' && next === '*') {
            const end = text.indexOf('*/', index + 2);
            const stop = end === -1 ? text.length : end + 2;
            masked += ' '.repeat(stop - index);
            index = stop - 1;
        } else {
            masked += char;
        }
    }
    return stripped === '' ? -1 : masked.indexOf('{');
}

/**
 * 설정 파일 내용에서 종료 시 동작을 읽는다. 키가 없거나 값이 ask·keep·stop이 아니면 ask다.
 *
 * Examples
 * --------
 * | 내용                                       | 결과   |
 * |--------------------------------------------|--------|
 * | `{"paddock.backend.onQuit": "keep"}`       | `keep` |
 * | `{"paddock.backend.onQuit": "later"}`      | `ask`  |
 * | `{}` · 빈 문자열                           | `ask`  |
 */
function readOnQuit(
    text,
) {
    const match = ENTRY.exec(stripComments(String(text || '')));
    let value = ON_QUIT.ASK;
    if (match && VALUES.includes(match[1])) value = match[1];
    return value;
}

/**
 * 설정 파일 내용에 종료 시 동작을 적은 새 내용을 돌려준다. 이미 키가 있으면 그 값만 바꾸고, 없으면 첫 `{` 바로 뒤에 더한다.
 * 결과가 주석을 뺀 JSON으로 읽히지 않으면(예: 파일이 객체가 아님) `null`이다. 이때는 파일을 쓰지 않는다.
 */
function withOnQuit(
    text,
    value,
) {
    const source = String(text || '');
    const entry = `"${KEY}": "${value}"`;
    let result = null;
    if (VALUES.includes(value)) {
        if (ENTRY.test(source)) {
            result = source.replace(ENTRY, entry);
        } else if (source.trim() === '') {
            result = `{\n    ${entry}\n}\n`;
        } else {
            // 첫 `{` 바로 뒤에 더한다. 마지막 항목 뒤에 주석이 있어도 쉼표 위치가 어긋나지 않는다.
            const open = openingBrace(source);
            if (open !== -1) {
                const rest = source.slice(open + 1);
                const isEmpty = stripComments(rest).replace(/\}\s*$/, '').trim() === '';
                result = `${source.slice(0, open + 1)}\n    ${entry}${isEmpty ? '' : ','}${rest}`;
            }
        }
        if (result !== null) {
            try {
                JSON.parse(stripComments(result).replace(/,(\s*[}\]])/g, '$1'));
            } catch {
                result = null;
            }
        }
    }
    return result;
}

module.exports = { KEY, ON_QUIT, readOnQuit, withOnQuit, stripComments };
