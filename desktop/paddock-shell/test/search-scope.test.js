const test = require('node:test');
const assert = require('node:assert/strict');
const { searchRoots } = require('../search-scope');

test('SS01: an opened workspace keeps its own roots', () => {
    assert.deepEqual(searchRoots({ workspaceRoots: ['file:///a', 'file:///b'], filesFolder: 'file:///f', topTerminalCwd: 'file:///c' }), ['file:///a', 'file:///b']);
});

test('SS02: without a workspace the folder of the file area is searched', () => {
    assert.deepEqual(searchRoots({ workspaceRoots: [], filesFolder: 'file:///f', topTerminalCwd: 'file:///c' }), ['file:///f']);
});

test('SS03: without a file area the current folder of the selected terminal is searched', () => {
    assert.deepEqual(searchRoots({ workspaceRoots: [], filesFolder: null, topTerminalCwd: 'file:///c' }), ['file:///c']);
});

test('SS04: with no folder at all there is nothing to search', () => {
    assert.deepEqual(searchRoots({ workspaceRoots: [], filesFolder: null, topTerminalCwd: null }), []);
});

test('SS05: the returned list is a copy of the workspace roots', () => {
    const roots = ['file:///a'];
    const result = searchRoots({ workspaceRoots: roots, filesFolder: null, topTerminalCwd: null });
    result.push('file:///x');
    assert.deepEqual(roots, ['file:///a']);
});
