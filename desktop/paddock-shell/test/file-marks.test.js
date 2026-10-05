const test = require('node:test');
const assert = require('node:assert/strict');
const { fileMarks, colorVariable, colorId } = require('../file-marks');

const MODIFIED = 'gitDecoration.modifiedResourceForeground';
const UNTRACKED = 'gitDecoration.untrackedResourceForeground';
const CONFLICT = 'gitDecoration.conflictingResourceForeground';

test('바뀐 파일은 상태 글자와 색을, 그 조상 폴더는 색을 받는다', () => {
    const marks = fileMarks([{ uri: 'file:///r/src/a.js', letter: 'M', color: MODIFIED }]);
    assert.deepEqual(marks.file('file:///r/src/a.js'), { letter: 'M', color: MODIFIED, tooltip: '' });
    assert.equal(marks.folder('file:///r/src'), MODIFIED);
    assert.equal(marks.folder('file:///r/src/'), MODIFIED, '끝의 / 는 무시한다');
    assert.equal(marks.folder('file:///r'), MODIFIED);
});

test('이름 앞부분만 같은 폴더와 바뀌지 않은 파일은 표시하지 않는다', () => {
    const marks = fileMarks([{ uri: 'file:///r/src/a.js', letter: 'M', color: MODIFIED }]);
    assert.equal(marks.folder('file:///r/s'), null);
    assert.equal(marks.folder('file:///r/src2'), null);
    assert.equal(marks.file('file:///r/src/b.js'), null);
    assert.equal(fileMarks([]).folder('file:///r'), null);
});

test('폴더에 변경이 섞이면 더 급한 색을 쓴다', () => {
    const marks = fileMarks([
        { uri: 'file:///r/a/new.js', letter: 'U', color: UNTRACKED },
        { uri: 'file:///r/a/old.js', letter: 'M', color: MODIFIED },
        { uri: 'file:///r/b/x.js', letter: 'M', color: MODIFIED },
        { uri: 'file:///r/b/y.js', letter: '!', color: CONFLICT },
    ]);
    assert.equal(marks.folder('file:///r/a'), MODIFIED);
    assert.equal(marks.folder('file:///r/b'), CONFLICT);
    assert.equal(marks.folder('file:///r'), CONFLICT);
});

test('같은 파일이 두 번 오면 먼저 온 표시를 쓴다', () => {
    const marks = fileMarks([
        { uri: 'file:///r/a.js', letter: 'M', color: MODIFIED },
        { uri: 'file:///r/a.js', letter: 'A', color: 'gitDecoration.addedResourceForeground' },
    ]);
    assert.equal(marks.file('file:///r/a.js').letter, 'M');
});

test('색 이름은 화면 색 변수로 바뀐다', () => {
    assert.equal(colorVariable(MODIFIED), 'var(--theia-gitDecoration-modifiedResourceForeground, #e2c08d)');
    assert.equal(colorVariable(''), '');
});

test('색 없이 글자만 오면 글자로 색을 정한다', () => {
    const marks = fileMarks([{ uri: 'file:///r/a.js', letter: 'M' }, { uri: 'file:///r/n.md', letter: 'U' }]);
    assert.equal(marks.file('file:///r/a.js').color, MODIFIED);
    assert.equal(marks.file('file:///r/n.md').color, UNTRACKED);
    assert.equal(marks.folder('file:///r'), MODIFIED);
});

test('Theia가 넘기는 화면 값 색도 색 이름으로 맞춘다', () => {
    assert.equal(colorId('var(--theia-gitDecoration-modifiedResourceForeground)'), MODIFIED);
    assert.equal(colorId(MODIFIED), MODIFIED);
    assert.equal(colorId('red'), '');
    const marks = fileMarks([{ uri: 'file:///r/a.js', letter: 'M', color: 'var(--theia-gitDecoration-modifiedResourceForeground)' }]);
    assert.equal(colorVariable(marks.file('file:///r/a.js').color), 'var(--theia-gitDecoration-modifiedResourceForeground, #e2c08d)');
});
