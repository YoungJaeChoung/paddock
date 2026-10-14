// 앱을 다시 열 때 지난 터미널을 되살릴지 정하는 설정(paddock.terminal.restoreOnRestart)을 설정 파일(settings.json)에서 읽는다.
// 설정 파일은 사용자가 손으로 고치는 주석 섞인 JSON이라, 읽을 때 이 키의 값만 찾는다. 앞 화면과 백엔드가 같은 판정을 쓴다.
const { stripComments } = require('./quit-preference');

/** 설정 키. 값은 ALWAYS·NEVER 중 하나다. */
const KEY = 'paddock.terminal.restoreOnRestart';

const RESTORE = Object.freeze({ ALWAYS: 'always', NEVER: 'never' });

const ENTRY = new RegExp(`"${KEY.replace(/\./g, '\\.')}"\\s*:\\s*"([^"\\n]*)"`);

/**
 * 설정 파일 내용에서 되살리기 설정을 읽는다. 키가 없거나 알 수 없는 값이면 기본값인 `always`다.
 *
 * Examples
 * --------
 * | 설정 파일 내용                                      | 결과     |
 * |-----------------------------------------------------|----------|
 * | `{ "paddock.terminal.restoreOnRestart": "never" }`  | `never`  |
 * | `{ "paddock.terminal.restoreOnRestart": "x" }` · `{}` · 빈 내용 | `always` |
 * | `// "paddock.terminal.restoreOnRestart": "never"`   | `always` |
 */
function readRestoreOnRestart(
    settingsText,
) {
    const found = ENTRY.exec(stripComments(String(settingsText || '')));
    return found && found[1] === RESTORE.NEVER ? RESTORE.NEVER : RESTORE.ALWAYS;
}

module.exports = { KEY, RESTORE, readRestoreOnRestart };
