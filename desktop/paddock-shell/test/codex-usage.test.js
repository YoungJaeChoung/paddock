const test = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { PassThrough } = require('node:stream');
const childProcess = require('node:child_process');
const { CodexUsage, codexUsageCommand, rateLimitWindows } = require('../codex-usage');
const usage = require('../usage-model');

function server(
    context,
    reply,
) {
    const calls = [];
    context.mock.method(childProcess, 'spawn', (command, argumentsList, options) => {
        const child = new EventEmitter();
        child.stdin = new PassThrough();
        child.stdout = new PassThrough();
        child.stderr = new PassThrough();
        child.kill = context.mock.fn(() => { child.emit('close', 0); return true; });
        child.stdin.on('finish', () => child.emit('close', 0));
        const call = { command, argumentsList, options, child, requests: [] };
        calls.push(call);
        child.stdin.on('data', data => {
            for (const line of data.toString().trim().split('\n')) {
                const request = JSON.parse(line);
                call.requests.push(request);
                if (request.method === 'initialize') child.stdout.write(`${JSON.stringify({ id: request.id, result: {} })}\n`);
                else if (request.method === 'account/rateLimits/read') reply(child, request, calls.length);
            }
        });
        return child;
    });
    return calls;
}

const scope = { profile: { id: 'fixture-account', runtime: 'native' }, configDir: '/profiles/fixture/codex' };
const limits = { rateLimitsByLimitId: { codex: { primary: { usedPercent: 0, windowDurationMins: 300, resetsAt: 1999999999 }, secondary: { usedPercent: 24.6, windowDurationMins: 10080, resetsAt: 1999999998 } }, other: { primary: { usedPercent: 99, windowDurationMins: 300 } } }, rateLimits: { primary: { usedPercent: 0, windowDurationMins: 300, resetsAt: 1999999999 }, secondary: { usedPercent: 24.6, windowDurationMins: 10080, resetsAt: 1999999998 } } };

test('CU01 official usage chooses the CLI default bucket, preserves zero, and returns only known numeric fields', () => {
    assert.deepEqual(rateLimitWindows(limits), [{ label: '5h', used: 0, resetsAt: 1999999999 }, { label: 'week', used: 25, resetsAt: 1999999998 }]);
    assert.deepEqual(rateLimitWindows({ rateLimits: { primary: { usedPercent: 'bad', windowDurationMins: 300 } } }), []);
    assert.deepEqual(rateLimitWindows({ rateLimits: { primary: { usedPercent: 14, windowDurationMins: 60, resetsAt: null } } }), [{ label: '1h', used: 14 }]);
});

test('CU02 initialized precedes rateLimits/read and successful results are cached by account', async context => {
    const calls = server(context, (child, request) => {
        const response = `${JSON.stringify({ id: request.id, result: limits })}\n`;
        child.stdout.write(response.slice(0, 30));
        child.stdout.write(response.slice(30));
    });
    const reader = new CodexUsage();
    const [first, second] = await Promise.all([reader.read(scope), reader.read(scope)]);
    assert.deepEqual(first, second);
    assert.equal(first.windows[0].used, 0);
    assert.equal(calls.length, 1);
    assert.deepEqual(calls[0].requests.map(item => item.method), ['initialize', 'initialized', 'account/rateLimits/read']);
    await reader.read(scope);
    assert.equal(calls.length, 1);
    await reader.read({ ...scope, configDir: '/profiles/other/codex' });
    assert.equal(calls.length, 2);
    assert.equal(calls[0].child.stdin.writableEnded, true);
    assert.equal(calls[0].child.kill.mock.callCount(), 0);
});

test('CU03 account environment removes alternate authentication selectors and WSL uses the saved distribution', context => {
    const selectors = ['OPENAI_API_KEY', 'CODEX_AUTH_JSON', 'ANTHROPIC_API_KEY'];
    const previous = Object.fromEntries(selectors.map(name => [name, process.env[name]]));
    for (const name of selectors) process.env[name] = 'synthetic-selector-value';
    context.after(() => { for (const name of selectors) { if (previous[name] === undefined) delete process.env[name]; else process.env[name] = previous[name]; } });
    const native = codexUsageCommand(scope);
    assert.equal(native.options.env.CODEX_HOME, scope.configDir);
    for (const name of ['OPENAI_API_KEY', 'CODEX_AUTH_JSON', 'ANTHROPIC_API_KEY']) assert.equal(Object.hasOwn(native.options.env, name), false);
    const wsl = codexUsageCommand({ profile: { runtime: 'wsl', wslDistribution: 'Debian Saved' }, configDir: "/home/person's data/codex" });
    assert.equal(wsl.command, 'wsl.exe');
    assert.deepEqual(wsl.argumentsList.slice(0, 4), ['--distribution', 'Debian Saved', '--exec', '/bin/bash']);
    assert.equal(wsl.argumentsList.at(-1), "/home/person's data/codex");
    assert.match(wsl.argumentsList.at(-3), /export CODEX_HOME="\$1"/);
    assert.match(wsl.argumentsList.at(-3), /unset .*OPENAI_API_KEY/);
});

test('CU04 official server errors never expose raw CLI output', async context => {
    const calls = server(context, (child, request, n_calls) => {
        child.stdout.write(`${JSON.stringify({ id: request.id, error: { code: -32000, message: 'private-value-must-not-escape' } })}\n`);
    });
    await assert.rejects(new CodexUsage().read(scope), error => error.statusCode === 503 && !error.message.includes('private-value'));
    assert.equal(calls.length, 1);
});

test('CU05 a weekly primary is shown as weekly and an absent secondary does not create a five-hour limit', () => {
    assert.deepEqual(rateLimitWindows({ rateLimits: { primary: { usedPercent: 83, windowDurationMins: 10080, resetsAt: 1791690576 }, secondary: null } }), [{ label: 'week', used: 83, resetsAt: 1791690576 }]);
    assert.deepEqual(rateLimitWindows({ rateLimits: null, rateLimitsByLimitId: { other: limits.rateLimits } }), []);
});

test('CU06 two-minute expiry refreshes the same account without keeping an earlier response indefinitely', async context => {
    context.mock.timers.enable({ apis: ['Date'], now: 1000000 });
    const calls = server(context, (child, request) => child.stdout.write(`${JSON.stringify({ id: request.id, result: limits })}\n`));
    const reader = new CodexUsage();
    await reader.read(scope);
    context.mock.timers.setTime(1119999);
    await reader.read(scope);
    assert.equal(calls.length, 1);
    context.mock.timers.setTime(1120000);
    await reader.read(scope);
    assert.equal(calls.length, 2);
});

test('CU07 a stalled lookup times out, closes input, and reaps only its own unresponsive child', async context => {
    context.mock.timers.enable({ apis: ['setTimeout'] });
    const calls = server(context, () => {});
    const reader = new CodexUsage();
    const pending = reader.read(scope);
    const rejected = assert.rejects(pending, /took too long/);
    calls[0].child.stdin.removeAllListeners('finish');
    context.mock.timers.tick(CodexUsage.TIMEOUT_MS);
    await rejected;
    assert.equal(calls[0].child.stdin.writableEnded, true);
    context.mock.timers.tick(CodexUsage.CLEANUP_MS);
    assert.equal(calls[0].child.kill.mock.callCount(), 1);
});

test('CU08 default CLI lookup keeps a healthy environment when another environment fails', async context => {
    const reader = new CodexUsage();
    context.mock.method(reader, 'read', async input => {
        if (input.configDir === '/missing') throw new Error('private failure');
        return { windows: rateLimitWindows(limits), updatedAt: 100 };
    });
    const value = await reader.readAvailable([{ profile: { runtime: 'native' }, configDir: '/missing', sessionsDirectory: '/not-created-native-sessions' }, { ...scope, sessionsDirectory: '/not-created-wsl-sessions' }]);
    assert.equal(value.windows[0].used, 0);
    assert.equal(Object.hasOwn(value, 'error'), false);
    const failed = await reader.readAvailable([{ profile: { runtime: 'native' }, configDir: '/missing', sessionsDirectory: '/not-created-native-sessions' }]);
    assert.deepEqual(failed.windows, []);
    assert.match(failed.error, /could not be read/);
    assert.equal(failed.error.includes('private'), false);
});

test('CU09 two default environments with usage require account selection instead of guessing from response times', async context => {
    const reader = new CodexUsage();
    context.mock.method(reader, 'read', async input => ({ windows: [{ label: 'week', used: input.profile.runtime === 'wsl' ? 80 : 20, resetsAt: 1999999999 }], updatedAt: Date.now() }));
    const value = await reader.readAvailable([{ ...scope, sessionsDirectory: '/not-created-native-sessions' }, { profile: { runtime: 'wsl' }, configDir: '/wsl/codex', sessionsDirectory: '/not-created-wsl-sessions' }]);
    assert.deepEqual(value.windows, []);
    assert.match(value.error, /saved account/);
});


test('CU10 an unreported reset time stays unknown while a confirmed elapsed reset is displayed separately', () => {
    const [unknown] = rateLimitWindows({ rateLimits: { primary: { usedPercent: 14, windowDurationMins: 60, resetsAt: null } } });
    assert.equal(unknown.resetsAt, undefined);
    assert.equal(usage.describe(unknown, 200), '14% used · 86% left');
    const [elapsed] = usage.currentWindows(rateLimitWindows({ rateLimits: { primary: { usedPercent: 14, windowDurationMins: 60, resetsAt: 100 } } }), 200);
    assert.equal(elapsed.resetsAt, null);
    assert.equal(elapsed.used, 0);
    assert.match(usage.describe(elapsed, 200), /next request|new window/i);
});
