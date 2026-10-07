const test = require('node:test');
const assert = require('node:assert/strict');
const { isBarePathMark, withoutBarePathLinks } = require('../terminal-links');

test('TL01: a lone dot, double dot or tilde is not a path link', () => {
    for (const text of ['.', '..', '~', '.:12']) assert.equal(isBarePathMark(text), true, text);
    for (const text of ['./src', '~/notes.md', 'orders.js', 'src/orders.js:3']) assert.equal(isBarePathMark(text), false, text);
});

test('TL02: the wrapped provider drops sentence-ending dots and keeps real paths', async () => {
    const line = '● Fixed it. See src/orders.js';
    const provider = {
        provideLinks: async () => [
            { startIndex: line.indexOf('.'), length: 1 },
            { startIndex: line.indexOf('src'), length: 'src/orders.js'.length },
        ],
    };
    withoutBarePathLinks(provider);
    withoutBarePathLinks(provider);
    const links = await provider.provideLinks(line, null);
    assert.deepEqual(links.map(link => line.substr(link.startIndex, link.length)), ['src/orders.js']);
});
