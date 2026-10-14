const test = require('node:test');
const assert = require('node:assert/strict');
const { withLocalReadme, isLocalUrl } = require('../extension-readme');

const LOCAL = 'http://localhost:38053/';

function fakeModel() {
    const calls = [];
    return { calls, model: { request: { request: async options => { calls.push(options.url); return { res: { statusCode: 200 }, buffer: new Uint8Array() }; } } } };
}

test('ER01: a README on the app backend is fetched directly, other URLs still go through the request service', async () => {
    const { calls, model } = fakeModel();
    const fetched = [];
    const fetchUrl = async url => { fetched.push(url); return new Response('# Title', { status: 200, headers: { 'content-type': 'text/markdown' } }); };
    withLocalReadme(model, LOCAL, fetchUrl);

    const local = await model.request.request({ url: 'http://localhost:38053/hostedPlugin/a_b/readme.md' });
    assert.equal(local.res.statusCode, 200);
    assert.equal(new TextDecoder().decode(local.buffer), '# Title');
    assert.equal(local.res.headers['content-type'], 'text/markdown');
    await model.request.request({ url: 'https://open-vsx.org/api/a/b/1.0.0/file/readme.md' });

    assert.deepEqual(fetched, ['http://localhost:38053/hostedPlugin/a_b/readme.md']);
    assert.deepEqual(calls, ['https://open-vsx.org/api/a/b/1.0.0/file/readme.md']);
});

test('ER02: a failing local response keeps its status code so the model reports it as before', async () => {
    const { model } = fakeModel();
    withLocalReadme(model, LOCAL, async () => new Response('missing', { status: 404 }));
    const context = await model.request.request({ url: 'http://localhost:38053/hostedPlugin/a_b/readme.md' });
    assert.equal(context.res.statusCode, 404);
});

test('ER03: wrapping the same model twice keeps a single wrapper', () => {
    const { model } = fakeModel();
    withLocalReadme(model, LOCAL, fetch);
    const wrapped = model.request;
    withLocalReadme(model, LOCAL, fetch);
    assert.equal(model.request, wrapped);
});

test('ER04: only the same protocol, host and port count as the app backend', () => {
    assert.equal(isLocalUrl('http://localhost:38053/x', LOCAL), true);
    assert.equal(isLocalUrl('http://localhost:9999/x', LOCAL), false);
    assert.equal(isLocalUrl('https://localhost:38053/x', LOCAL), false);
    assert.equal(isLocalUrl('not a url', LOCAL), false);
    assert.equal(isLocalUrl(undefined, LOCAL), false);
});
