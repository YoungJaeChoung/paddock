const test = require('node:test');
const assert = require('node:assert/strict');
const { trustWslRepositories } = require('../git-environment');

test('Windows git trusts repositories under both WSL path prefixes', () => {
    assert.deepEqual(trustWslRepositories({}, 'win32'), {
        GIT_CONFIG_COUNT: '2',
        GIT_CONFIG_KEY_0: 'safe.directory',
        GIT_CONFIG_VALUE_0: '%(prefix)///wsl.localhost/*',
        GIT_CONFIG_KEY_1: 'safe.directory',
        GIT_CONFIG_VALUE_1: '%(prefix)///wsl$/*',
    });
});

test('existing command-line git settings keep their numbers', () => {
    const env = trustWslRepositories({ GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'core.autocrlf', GIT_CONFIG_VALUE_0: 'false' }, 'win32');
    assert.equal(env.GIT_CONFIG_COUNT, '3');
    assert.equal(env.GIT_CONFIG_KEY_0, 'core.autocrlf');
    assert.equal(env.GIT_CONFIG_VALUE_1, '%(prefix)///wsl.localhost/*');
    assert.equal(env.GIT_CONFIG_VALUE_2, '%(prefix)///wsl$/*');
});

test('Linux and macOS environments stay unchanged', () => {
    assert.deepEqual(trustWslRepositories({ PATH: '/usr/bin' }, 'linux'), { PATH: '/usr/bin' });
    assert.deepEqual(trustWslRepositories({}, 'darwin'), {});
});
