const test = require('node:test');
const assert = require('node:assert/strict');
const { relativePath, formatNotes } = require('../review-notes');

test('RN01: a path inside a work folder becomes relative and the folder boundary is respected', () => {
    assert.equal(relativePath('/repo/src/a.js', ['/repo']), 'src/a.js');
    assert.equal(relativePath('/repo/src/a.js', ['/repo/']), 'src/a.js');
    assert.equal(relativePath('/repo-other/a.js', ['/repo']), '/repo-other/a.js');
    assert.equal(relativePath('/x/a.js', []), '/x/a.js');
});

test('RN02: notes are numbered with file, line range and the first line text', () => {
    const text = formatNotes([
        { file: 'src/a.js', startLine: 3, endLine: 3, lineText: '  return x;', text: 'null check' },
        { file: 'b.js', startLine: 5, endLine: 9, lineText: 'if (a) {', text: 'split\nthis up' },
    ]);
    assert.equal(text, 'Review notes (2):\n\n1. src/a.js:3\n   > return x;\n   null check\n\n2. b.js:5-9\n   > if (a) {\n   split\n   this up\n');
});

test('RN03: no notes make no text and long lines are cut', () => {
    assert.equal(formatNotes([]), '');
    const text = formatNotes([{ file: 'a', startLine: 1, endLine: 1, lineText: 'x'.repeat(500), text: 't' }]);
    assert.ok(text.includes(`${'x'.repeat(160)}…`));
    assert.ok(!text.includes('x'.repeat(161)));
});
