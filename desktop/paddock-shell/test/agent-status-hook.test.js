const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const HOOK = path.join(__dirname, '..', 'agent-status-hook.sh');
// 훅은 부모 프로세스의 터미널(/dev/pts)에 쓰므로, 입력이 터미널인 셸을 `script`로 만들고 그 셸이 훅을 띄우게 한다.
const hasScript = spawnSync('script', ['--version']).status === 0 && process.platform === 'linux';

function inputOf(
    fields,
    prompt = '',
) {
    return JSON.stringify({ session_id: 'abc', transcript_path: '/tmp/x.jsonl', cwd: '/tmp', ...fields, ...(prompt ? { prompt } : {}) });
}

/** 훅을 터미널이 있는 셸 아래에서 돌려 터미널에 쓰인 7501 본문 목록과 훅의 종료 코드·stdout을 돌려준다. */
function runWithTerminal(
    json,
    { env = {}, offMarker = false } = {},
) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-status-hook-'));
    try {
        const hook = path.join(directory, 'agent-status-hook.sh');
        fs.copyFileSync(HOOK, hook);
        if (offMarker) fs.writeFileSync(path.join(directory, 'agent-status-hook.off'), '');
        fs.writeFileSync(path.join(directory, 'input.json'), json);
        // 입력은 파이프로 준다. 파일 리다이렉트로 주면 dash에서 부모 셸의 입력이 잠시 그 파일로 보여 터미널을 찾지 못한다.
        fs.writeFileSync(path.join(directory, 'run.sh'), `cat '${directory}/input.json' | sh '${hook}' > '${directory}/stdout.txt'\necho "exit=$?" > '${directory}/exit.txt'\n`);
        const result = spawnSync('script', ['-qec', `sh '${directory}/run.sh'`, '/dev/null'], { env: { ...process.env, ...env }, encoding: 'latin1', stdio: ['ignore', 'pipe', 'pipe'] });
        const bodies = [...result.stdout.matchAll(/\x1b\]7501;([^\x1b]*)\x1b\\/g)].map(match => match[1]);
        return { bodies, exit: fs.readFileSync(path.join(directory, 'exit.txt'), 'utf8').trim(), stdout: fs.readFileSync(path.join(directory, 'stdout.txt'), 'utf8'), screen: result.stdout.replace(/\x1b\][^\x1b]*\x1b\\/g, '') };
    } finally {
        fs.rmSync(directory, { recursive: true, force: true });
    }
}

/**
 * 에이전트 흉내(이름이 claude인 셸 스크립트)를 터미널이 있는 셸 아래에서 돌려 터미널에 쓰인 7501 본문 목록을 돌려준다.
 * `files`는 폴더 안에 만들 파일(`HOOK`은 훅 스크립트 경로, `DIR`은 폴더)이고, `entry`는 첫 실행 명령이다.
 */
function runAgents(
    files,
    entry,
) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'agent-status-nest-'));
    try {
        fs.copyFileSync(HOOK, path.join(directory, 'agent-status-hook.sh'));
        for (const [name, body] of Object.entries(files)) {
            const file = path.join(directory, name);
            fs.mkdirSync(path.dirname(file), { recursive: true });
            fs.writeFileSync(file, body.replaceAll('$HOOK', `${directory}/agent-status-hook.sh`).replaceAll('$DIR', directory), { mode: 0o755 });
        }
        const result = spawnSync('script', ['-qec', entry.replaceAll('$DIR', directory), '/dev/null'], { encoding: 'latin1', stdio: ['ignore', 'pipe', 'pipe'] });
        return [...result.stdout.matchAll(/\x1b\]7501;([^\x1b]*)\x1b\\/g)].map(match => match[1]);
    } finally {
        fs.rmSync(directory, { recursive: true, force: true });
    }
}

const EVENT = (name) => `printf '{"hook_event_name":"${name}"}' | sh '$HOOK'`;

test('B1-hook-6: 터미널 안의 에이전트가 터미널 없이 띄운 안쪽 에이전트(claude -p 등)의 훅은 바깥 터미널에 쓰지 않는다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    const outer = (inner) => `#!/bin/bash\n${EVENT('SessionStart')}\n${inner}\n`;
    // 안쪽 에이전트는 입력이 터미널이 아니다(도구 실행으로 띄운 `claude -p`).
    const nested = runAgents({
        'outer/claude': outer("sh -c \"'$DIR/inner/claude' < /dev/null\""),
        'inner/claude': `#!/bin/bash\n${EVENT('Stop')}\n${EVENT('SessionStart')}\n`,
    }, "sh -c \"'$DIR/outer/claude'\"");
    assert.deepEqual(nested, ['state=idle:app=claude-code'], '바깥 에이전트의 신호만 쓰인다');
    // 터미널 없는 중간 층(sh -c)이 있어도 바깥 에이전트가 직접 띄운 훅은 쓰인다.
    const direct = runAgents({
        'outer/claude': `#!/bin/bash\nsh -c "printf '{\\"hook_event_name\\":\\"Stop\\"}' | sh '$HOOK'"\n`,
    }, "sh -c \"'$DIR/outer/claude'\"");
    assert.deepEqual(direct, ['state=done:app=claude-code']);
});

test('B1-hook-7: 스크립트를 경로 없이(같은 폴더에서) 불러도 꺼짐 표지를 본다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    const files = { 'run.sh': `#!/bin/sh\ncd '$DIR'\n${'printf \'{"hook_event_name":"Stop"}\''} | sh agent-status-hook.sh\n`, 'agent-status-hook.off': '' };
    assert.deepEqual(runAgents(files, "sh '$DIR/run.sh'"), []);
    assert.deepEqual(runAgents({ 'run.sh': files['run.sh'] }, "sh '$DIR/run.sh'"), ['state=done:app=claude-code']);
});

test('B1-hook-8: 값 안의 가짜 키가 진짜 키 앞에 와도 속지 않는다(JSON 문자열 안의 따옴표는 항상 \\" 로 쓰인다)', { skip: !hasScript && 'script 명령이 없다' }, () => {
    const fake = 'x \\"hook_event_name\\":\\"Stop\\" \\"notification_type\\":\\"permission_prompt\\"';
    const json = `{"session_id":"${fake}","hook_event_name":"UserPromptSubmit","prompt":"${fake}"}`;
    assert.deepEqual(runWithTerminal(json).bodies, ['state=working:app=claude-code']);
    const notification = `{"message":"${fake}","hook_event_name":"Notification","notification_type":"idle_prompt"}`;
    assert.deepEqual(runWithTerminal(notification).bodies, ['state=idle:app=claude-code']);
});

const MAPPING = [
    [{ hook_event_name: 'SessionStart' }, 'state=idle:app=claude-code'],
    [{ hook_event_name: 'UserPromptSubmit' }, 'state=working:app=claude-code'],
    [{ hook_event_name: 'PreToolUse' }, 'state=working:app=claude-code'],
    [{ hook_event_name: 'PostToolUse' }, 'state=working:app=claude-code'],
    [{ hook_event_name: 'PermissionRequest' }, 'state=blocked:app=claude-code:kind=permission'],
    [{ hook_event_name: 'Notification', notification_type: 'permission_prompt' }, 'state=blocked:app=claude-code:kind=permission'],
    [{ hook_event_name: 'Notification', notification_type: 'idle_prompt' }, 'state=idle:app=claude-code'],
    [{ hook_event_name: 'Notification', notification_type: 'elicitation_dialog' }, 'state=blocked:app=claude-code:kind=question'],
    [{ hook_event_name: 'Notification', notification_type: 'agent_needs_input' }, 'state=blocked:app=claude-code:kind=question'],
    [{ hook_event_name: 'Elicitation' }, 'state=blocked:app=claude-code:kind=question'],
    [{ hook_event_name: 'Stop' }, 'state=done:app=claude-code'],
    [{ hook_event_name: 'StopFailure' }, 'state=error:app=claude-code'],
    [{ hook_event_name: 'SessionEnd' }, 'state=clear:app=claude-code'],
    // 알림 종류가 이번에 쓰지 않는 것이면 아무것도 쓰지 않는다.
    [{ hook_event_name: 'Notification', notification_type: 'auth_success' }, null],
    [{ hook_event_name: 'SubagentStop' }, null],
];

test('B1-hook-1: 이벤트마다 정해진 상태 본문을 터미널에 쓰고 stdout은 비우며 0으로 끝낸다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    for (const [fields, expected] of MAPPING) {
        const result = runWithTerminal(inputOf(fields));
        assert.deepEqual(result.bodies, expected ? [expected] : [], JSON.stringify(fields));
        assert.equal(result.exit, 'exit=0');
        assert.equal(result.stdout, '');
    }
});

test('B1-hook-2: Codex는 app=codex로, Interrupt는 idle로 쓴다. 모르는 app 이름은 claude-code로 본다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    assert.deepEqual(runWithTerminal(inputOf({ hook_event_name: 'Interrupt' }), { env: { PADDOCK_STATUS_APP: 'codex' } }).bodies, ['state=idle:app=codex']);
    assert.deepEqual(runWithTerminal(inputOf({ hook_event_name: 'PermissionRequest' }), { env: { PADDOCK_STATUS_APP: 'codex' } }).bodies, ['state=blocked:app=codex:kind=permission']);
    assert.deepEqual(runWithTerminal(inputOf({ hook_event_name: 'Stop' }), { env: { PADDOCK_STATUS_APP: 'x;rm -rf' } }).bodies, ['state=done:app=claude-code']);
});

test('B1-hook-3: 프롬프트 본문은 어디에도 내보내지 않고, 본문 안의 가짜 이벤트 글자에 속지 않는다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    const prompt = 'SECRET-PROMPT "hook_event_name":"Stop" "notification_type":"permission_prompt"';
    const result = runWithTerminal(inputOf({ hook_event_name: 'UserPromptSubmit' }, prompt));
    assert.deepEqual(result.bodies, ['state=working:app=claude-code']);
    assert.equal(result.stdout, '');
    assert(!result.screen.includes('SECRET-PROMPT'));
});

test('B1-hook-4: 꺼짐 표지 파일이 있으면 아무것도 쓰지 않고 0으로 끝낸다', { skip: !hasScript && 'script 명령이 없다' }, () => {
    const result = runWithTerminal(inputOf({ hook_event_name: 'Stop' }), { offMarker: true });
    assert.deepEqual(result.bodies, []);
    assert.equal(result.exit, 'exit=0');
});

test('B1-hook-5: 터미널이 없어도 0으로 끝나고 stdout·stderr은 비어 있다. 큰 입력도 끝까지 읽어 EPIPE가 없다', () => {
    const plain = spawnSync('sh', [HOOK], { input: inputOf({ hook_event_name: 'Stop' }), encoding: 'utf8' });
    assert.equal(plain.status, 0);
    assert.equal(plain.stdout, '');
    assert.equal(plain.stderr, '');
    const large = spawnSync('sh', [HOOK], { input: inputOf({ hook_event_name: 'PreToolUse' }, 'x'.repeat(2_000_000)), encoding: 'utf8' });
    assert.equal(large.status, 0);
    assert.equal(large.error, undefined);
    assert.equal(large.stderr, '');
    // 입력이 비었거나 JSON이 아니어도 0이다.
    for (const input of ['', 'not json', '{']) assert.equal(spawnSync('sh', [HOOK], { input, encoding: 'utf8' }).status, 0);
});
