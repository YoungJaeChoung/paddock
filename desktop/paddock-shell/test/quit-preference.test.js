const assert = require('node:assert/strict');
const { test } = require('node:test');
const { readOnQuit, withOnQuit, stripComments } = require('../quit-preference');

test('readOnQuit은 값이 없거나 잘못되면 ask다', () => {
    assert.equal(readOnQuit(''), 'ask');
    assert.equal(readOnQuit('{}'), 'ask');
    assert.equal(readOnQuit('{"paddock.backend.onQuit": "later"}'), 'ask');
    assert.equal(readOnQuit('{"paddock.backend.onQuit": "keep"}'), 'keep');
    assert.equal(readOnQuit('{\n  "paddock.backend.onQuit": "stop",\n}'), 'stop');
});

test('주석 안에 적힌 값은 읽지 않는다', () => {
    assert.equal(readOnQuit('{\n  // "paddock.backend.onQuit": "keep"\n}'), 'ask');
    assert.equal(readOnQuit('{ /* "paddock.backend.onQuit": "stop" */ }'), 'ask');
});

test('stripComments는 문자열 안의 슬래시를 지우지 않는다', () => {
    assert.equal(stripComments('{"a": "http://x"} // c'), '{"a": "http://x"} \n');
});

test('withOnQuit은 키가 있으면 값만 바꾸고 나머지 내용을 그대로 둔다', () => {
    const text = '{\n    // 내 설정\n    "a": 1,\n    "paddock.backend.onQuit": "ask"\n}\n';
    assert.equal(withOnQuit(text, 'keep'), text.replace('"ask"', '"keep"'));
});

test('withOnQuit은 키가 없으면 첫 중괄호 바로 뒤에 더한다', () => {
    assert.equal(
        withOnQuit('{\n    "a": 1\n}\n', 'stop'),
        '{\n    "paddock.backend.onQuit": "stop",\n    "a": 1\n}\n',
    );
    assert.equal(withOnQuit('{}', 'keep'), '{\n    "paddock.backend.onQuit": "keep"}');
    assert.equal(withOnQuit('', 'keep'), '{\n    "paddock.backend.onQuit": "keep"\n}\n');
});

test('withOnQuit은 마지막 항목 뒤에 주석이 있어도 쓸 수 있다', () => {
    const result = withOnQuit('{\n    "a": 1 // 끝\n}\n', 'keep');
    assert.equal(result, '{\n    "paddock.backend.onQuit": "keep",\n    "a": 1 // 끝\n}\n');
    assert.equal(readOnQuit(result), 'keep');
});

test('withOnQuit은 앞쪽 주석의 중괄호를 건너뛴다', () => {
    const result = withOnQuit('// {예시}\n{\n    "a": 1\n}\n', 'stop');
    assert.equal(result, '// {예시}\n{\n    "paddock.backend.onQuit": "stop",\n    "a": 1\n}\n');
});

test('withOnQuit은 객체가 아니거나 값이 잘못되면 null이다', () => {
    assert.equal(withOnQuit('{}', 'later'), null);
    assert.equal(withOnQuit('not json', 'keep'), null);
});
