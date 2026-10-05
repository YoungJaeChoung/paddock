const test = require('node:test');
const assert = require('node:assert/strict');
const { AgentAccount, parseEnviron, normalizePath, attributeAccount, runningScope, scopeName } = require('../agent-account');

const CONFIG = '/home/me/.paddock/config';
const WORK_CODEX = { id: '11111111-1111-4111-8111-111111111111', provider: 'codex', label: 'Work Codex', runtime: 'native' };
const WORK_CLAUDE = { id: '22222222-2222-4222-8222-222222222222', provider: 'claude', label: 'Work Claude', runtime: 'native' };
const WSL_CODEX = { id: '33333333-3333-4333-8333-333333333333', provider: 'codex', label: 'WSL Codex', runtime: 'wsl', wslDistribution: 'Ubuntu' };
const PROFILES = [WORK_CODEX, WORK_CLAUDE, WSL_CODEX];

function nativeDirectory(
    profile,
) {
    return `${CONFIG}/agent-profiles/${profile.id}/${profile.provider}`;
}

function attribute(
    provider,
    env,
    options = {},
) {
    return attributeAccount({ provider, runtime: 'native', env, profiles: PROFILES, profileDirectory: nativeDirectory, ...options });
}

test('AA01 environ에서 계정 판정에 쓰는 세 값만 꺼내고 로그인 키 값은 버린다', () => {
    const env = parseEnviron('HOME=/home/me\0CODEX_HOME=/x\0OPENAI_API_KEY=secret\0CLAUDE_CONFIG_DIR=\0PATH=/bin\0');
    assert.deepEqual(env, { HOME: '/home/me', CODEX_HOME: '/x' });
    assert.deepEqual(parseEnviron('CLAUDE_CONFIG_DIR=/c\x1fANTHROPIC_API_KEY=k\x1f', '\x1f'), { CLAUDE_CONFIG_DIR: '/c' });
    assert.deepEqual(parseEnviron(''), {});
    assert.deepEqual(parseEnviron(undefined), {});
});

test('AA02 비교용 경로는 겹친 구분자·점·끝 구분자를 정리하고 상대 경로는 비운다', () => {
    assert.equal(normalizePath('/home//me/./.codex/'), '/home/me/.codex');
    assert.equal(normalizePath('/home/me/x/../.codex'), '/home/me/.codex');
    assert.equal(normalizePath('/'), '/');
    assert.equal(normalizePath('relative/.codex'), '');
    assert.equal(normalizePath(''), '');
});

test('AA03 계정 변수가 없거나 기본 폴더면 기본 CLI다', () => {
    const DEFAULT = { kind: AgentAccount.KINDS.DEFAULT };
    assert.deepEqual(attribute('codex', { HOME: '/home/me' }), DEFAULT);
    assert.deepEqual(attribute('codex', {}), DEFAULT);
    assert.deepEqual(attribute('codex', { HOME: '/home/me', CODEX_HOME: '/home/me/.codex/' }), DEFAULT);
    assert.deepEqual(attribute('claude', { HOME: '/home/me', CLAUDE_CONFIG_DIR: '/home/me/.claude' }), DEFAULT);
    // 앱이 물려준 기본 폴더(앱 자체의 CODEX_HOME)도 기본 CLI다.
    assert.deepEqual(attribute('codex', { HOME: '/home/me', CODEX_HOME: '/opt/codex' }, { defaultDirectories: ['/opt/codex'] }), DEFAULT);
    // 다른 도구의 변수만 있으면 이 도구는 기본 폴더를 쓴다(Claude 계정 터미널에서 실행한 codex).
    assert.deepEqual(attribute('codex', { HOME: '/home/me', CLAUDE_CONFIG_DIR: nativeDirectory(WORK_CLAUDE) }), DEFAULT);
});

test('AA04 등록 계정 폴더와 실제 경로가 같고 도구·환경이 맞으면 그 계정이다', () => {
    assert.deepEqual(attribute('codex', { HOME: '/home/me', CODEX_HOME: nativeDirectory(WORK_CODEX) }), { kind: 'profile', profile: WORK_CODEX });
    assert.deepEqual(attribute('claude', { CLAUDE_CONFIG_DIR: `${nativeDirectory(WORK_CLAUDE)}/` }), { kind: 'profile', profile: WORK_CLAUDE });
    // 심볼릭 링크로 가리킨 폴더는 실제 경로로 비교한다.
    const realPath = file => (file === '/home/me/link' ? nativeDirectory(WORK_CODEX) : file);
    assert.deepEqual(attribute('codex', { CODEX_HOME: '/home/me/link' }, { realPath }), { kind: 'profile', profile: WORK_CODEX });
});

test('AA05 그 밖의 폴더는 미등록 폴더이고 다른 도구·환경의 계정 폴더도 빌려 쓰지 않는다', () => {
    const CUSTOM = { kind: AgentAccount.KINDS.CUSTOM };
    assert.deepEqual(attribute('codex', { HOME: '/home/me', CODEX_HOME: '/tmp/other' }), CUSTOM);
    assert.deepEqual(attribute('codex', { CODEX_HOME: 'relative/codex' }), CUSTOM);
    // Claude 계정 폴더를 CODEX_HOME으로 지정한 경우.
    assert.deepEqual(attribute('codex', { CODEX_HOME: `${CONFIG}/agent-profiles/${WORK_CLAUDE.id}/codex` }), CUSTOM);
    // WSL 계정은 native 환경에서 같은 경로라도 맞지 않는다.
    assert.deepEqual(attribute('codex', { CODEX_HOME: nativeDirectory(WSL_CODEX) }), CUSTOM);
    assert.equal(attribute('bash', { CODEX_HOME: nativeDirectory(WORK_CODEX) }), null);
    assert.equal(attribute(null, {}), null);
});

test('AA06 WSL 계정은 그 배포판의 홈 아래 폴더로 맞추고, 배포판이 다르면 미등록 폴더다', () => {
    const wslDirectory = profile => `/home/u/.paddock/agent-profiles/${profile.id}/${profile.provider}`;
    const env = { HOME: '/home/u', CODEX_HOME: wslDirectory(WSL_CODEX) };
    const base = { provider: 'codex', runtime: 'wsl', env, profiles: PROFILES, profileDirectory: wslDirectory };
    assert.deepEqual(attributeAccount({ ...base, wslDistribution: 'ubuntu' }), { kind: 'profile', profile: WSL_CODEX });
    assert.deepEqual(attributeAccount({ ...base }), { kind: 'profile', profile: WSL_CODEX });
    assert.deepEqual(attributeAccount({ ...base, wslDistribution: 'Debian' }), { kind: 'custom' });
    assert.deepEqual(attributeAccount({ ...base, env: { HOME: '/home/u' } }), { kind: 'default' });
});

test('AA07 화면 범위는 실행 중인 도구의 계정이 이기고, 도구가 없으면 터미널을 연 계정이다', () => {
    const cases = [
        // [program, launchProfile, detected, expected]
        ['bash', WORK_CLAUDE, undefined, WORK_CLAUDE],
        ['claude', WORK_CLAUDE, { kind: 'profile', profile: { ...WORK_CLAUDE } }, WORK_CLAUDE],
        ['codex', WORK_CLAUDE, { kind: 'default' }, { provider: 'codex' }],
        ['codex', WORK_CLAUDE, undefined, { provider: 'codex' }],
        ['claude', WORK_CLAUDE, undefined, WORK_CLAUDE],
        ['codex', undefined, { kind: 'profile', profile: WORK_CODEX }, WORK_CODEX],
        ['codex', WORK_CLAUDE, { kind: 'profile', profile: WORK_CODEX }, WORK_CODEX],
        ['codex', undefined, { kind: 'custom' }, { provider: 'codex', custom: true }],
        ['claude', undefined, { kind: 'default' }, { provider: 'claude' }],
        ['claude', undefined, undefined, { provider: 'claude' }],
        ['bash', undefined, undefined, null],
        [undefined, undefined, undefined, null],
    ];
    for (const [program, launchProfile, detected, expected] of cases) {
        assert.deepEqual(runningScope({ program, launchProfile, detected }), expected, `${program} / ${launchProfile?.label} / ${detected?.kind}`);
    }
    // 계정 터미널은 같은 계정이면 터미널이 가진 계정 객체 그대로다(이름 변경이 반영된 쪽).
    assert.equal(runningScope({ program: 'claude', launchProfile: WORK_CLAUDE, detected: { kind: 'profile', profile: { ...WORK_CLAUDE } } }), WORK_CLAUDE);
});

test('AA08 범위 이름은 계정 이름, 미등록 폴더, Current CLI 중 하나다', () => {
    assert.equal(scopeName(WORK_CODEX), 'Work Codex');
    assert.equal(scopeName({ provider: 'codex', custom: true }), 'Unregistered folder');
    assert.equal(scopeName({ provider: 'codex' }), 'Default');
});

test('AA09 계정 터미널에서 같은 도구를 다른 설정 폴더로 실행하면 터미널을 연 계정이 아니라 감지 결과를 따른다', () => {
    // 계정 전환 요청(accountSessionRequest)도 이 범위를 쓰므로, 상태 줄과 전환 출처가 같은 계정을 가리킨다.
    const OTHER_CLAUDE = { id: '44444444-4444-4444-8444-444444444444', provider: 'claude', label: 'Other Claude', runtime: 'native' };
    assert.deepEqual(runningScope({ program: 'claude', launchProfile: WORK_CLAUDE, detected: { kind: 'default' } }), { provider: 'claude' });
    assert.equal(runningScope({ program: 'claude', launchProfile: WORK_CLAUDE, detected: { kind: 'profile', profile: OTHER_CLAUDE } }), OTHER_CLAUDE);
    assert.deepEqual(runningScope({ program: 'claude', launchProfile: WORK_CLAUDE, detected: { kind: 'custom' } }), { provider: 'claude', custom: true });
});

test('AA10 기본 CLI의 로그인이 등록 계정 하나로 확인되면 범위는 기본 CLI 그대로 두고 이름만 그 계정이다', () => {
    const scope = runningScope({ program: 'codex', launchProfile: undefined, detected: { kind: 'default', profile: WORK_CODEX } });
    assert.deepEqual(scope, { provider: 'codex', linked: { id: WORK_CODEX.id, label: 'Work Codex' } });
    assert.equal(scope.id, undefined, '대화 기록·계정 전환은 기본 폴더를 쓴다');
    assert.equal(scopeName(scope), 'Work Codex');
    // 계정 터미널에서 기본 codex를 실행해도 같다.
    assert.deepEqual(runningScope({ program: 'codex', launchProfile: WORK_CLAUDE, detected: { kind: 'default', profile: WORK_CODEX } }).linked.id, WORK_CODEX.id);
    // 다른 도구의 계정은 연결 근거가 아니다.
    assert.deepEqual(runningScope({ program: 'claude', launchProfile: undefined, detected: { kind: 'default', profile: WORK_CODEX } }), { provider: 'claude' });
});
