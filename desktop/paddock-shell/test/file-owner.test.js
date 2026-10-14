const test = require('node:test');
const assert = require('node:assert/strict');
const { fileRootFor } = require('../file-owner');

const state = { folders: [{ key: 'file:///home/me/repo' }] };
const base = { state, uri: 'file:///tmp/x.ts', hasRoot: false, currentRoot: null, topTerminal: 't2' };

test('FO01: a file outside every work folder joins the group of the current screen first', () => {
    assert.equal(fileRootFor({ ...base, currentRoot: 'terminal-1' }), 'terminal-1');
});

test('FO02: without a current group the shown top terminal becomes the group', () => {
    assert.equal(fileRootFor(base), 't2');
});

test('FO03: a file inside a work folder needs no group', () => {
    assert.equal(fileRootFor({ ...base, uri: 'file:///home/me/repo/a.ts' }), null);
});

test('FO04: a screen that already has a group or no file address keeps as is', () => {
    assert.equal(fileRootFor({ ...base, hasRoot: true }), null);
    assert.equal(fileRootFor({ ...base, uri: null }), null);
    assert.equal(fileRootFor({ ...base, topTerminal: null }), null);
});

test('FO05: a screen whose area is not yet known counts as body, but a known non-body area gets no group', () => {
    // 검색 결과로 여는 미리보기 편집기는 들어왔다는 신호가 영역이 정해지기 전에 온다.
    assert.equal(fileRootFor({ ...base, area: null }), 't2');
    assert.equal(fileRootFor({ ...base, area: undefined }), 't2');
    assert.equal(fileRootFor({ ...base, area: 'main' }), 't2');
    for (const area of ['left', 'right', 'bottom', 'secondaryWindow']) assert.equal(fileRootFor({ ...base, area }), null);
});
