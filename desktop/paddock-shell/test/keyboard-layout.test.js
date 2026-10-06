const test = require('node:test');
const assert = require('node:assert/strict');
const { isLatinLayout } = require('../keyboard-layout');

function layout(
    letters,
) {
    return Object.fromEntries([...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].map((letter, index) => [`Key${letter}`, { value: letters[index] }]));
}

test('C-keyboard-layout: 글자 키가 라틴 글자를 내지 않는 배열만 골라낸다', () => {
    assert.equal(isLatinLayout(layout('abcdefghijklmnopqrstuvwxyz')), true);
    assert.equal(isLatinLayout(layout('qbcdefghijkl,nopazstuvwxy')), true);
    assert.equal(isLatinLayout(layout('ㅁㅠㅊㅇㄷㄹㅎㅗㅑㅓㅏㅣㅡㅜㅐㅔㅂㄱㄴㅅㅕㅍㅈㅌㅛㅋ')), false);
    assert.equal(isLatinLayout(layout('фисвуапршолдьтщзйкыегмцчня')), false);
    assert.equal(isLatinLayout({}), true);
    assert.equal(isLatinLayout(undefined), true);
});
