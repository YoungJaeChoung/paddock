const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const SHELL_DIRECTORY = path.join(__dirname, '..');
const SIZE_LIMIT_PROPERTIES = ['min-width', 'max-width', 'min-height', 'max-height'];

// 상태 줄 자신을 고르는 규칙(자손 선택자가 아닌 것)과 그 규칙이 media 블록 안에 있는지를 모은다.
function statusbarRules() {
    const rules = [];
    for (const name of fs.readdirSync(SHELL_DIRECTORY).filter(file => file.endsWith('.css'))) {
        const css = fs.readFileSync(path.join(SHELL_DIRECTORY, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
        let n_open_media = 0;
        const blockKinds = [];
        const tokenPattern = /([^{}]*)([{}])/g;
        let match;
        while ((match = tokenPattern.exec(css))) {
            const prelude = match[1].trim();
            if (match[2] === '{') {
                const isMedia = prelude.startsWith('@media');
                blockKinds.push(isMedia ? 'media' : 'rule');
                n_open_media += isMedia ? 1 : 0;
                const selectsStatusbar = !isMedia && prelude.split(',').some(selector => /\.statusbar\s*$/.test(selector.trim()));
                if (selectsStatusbar) {
                    const end = css.indexOf('}', tokenPattern.lastIndex);
                    rules.push({ file: name, selector: prelude, body: css.slice(tokenPattern.lastIndex, end), inMedia: n_open_media > 0 });
                }
            } else {
                n_open_media -= blockKinds.pop() === 'media' ? 1 : 0;
            }
        }
    }
    return rules;
}

function sizeLimits(
    body,
) {
    return body.split(';')
        .map(declaration => declaration.split(':').map(part => part.trim()))
        .filter(([property]) => SIZE_LIMIT_PROPERTIES.includes(property));
}

test('C-statusbar-css.1: 상태 줄의 크기 상한·하한은 창 크기와 무관한 고정값이다', () => {
    const rules = statusbarRules();
    assert.ok(rules.length > 0, '상태 줄 규칙을 찾지 못했다');
    for (const rule of rules) {
        for (const [property, value] of sizeLimits(rule.body)) {
            assert.doesNotMatch(value, /vw|vh|vmin|vmax|%/, `${rule.file} "${rule.selector}" ${property}: ${value}`);
        }
    }
});

test('C-statusbar-css.2: media 규칙은 상태 줄의 크기 상한·하한을 바꾸지 않는다', () => {
    for (const rule of statusbarRules().filter(candidate => candidate.inMedia)) {
        assert.deepEqual(sizeLimits(rule.body), [], `${rule.file} "${rule.selector}"`);
    }
});
