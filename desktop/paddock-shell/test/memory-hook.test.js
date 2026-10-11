const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
const { execFileSync } = require('node:child_process');

const HOOK = path.join(__dirname, '..', 'memory-hook.sh');

/** 훅을 실행해 stdout을 돌려준다. 기준값은 사용자 설정(PADDOCK_MEMORY_MIN_MB)으로만 바꾼다. */
function run(
    toolName,
    minimumMb,
) {
    const env = { ...process.env };
    if (minimumMb === undefined) delete env.PADDOCK_MEMORY_MIN_MB;
    else env.PADDOCK_MEMORY_MIN_MB = String(minimumMb);
    return execFileSync('sh', [HOOK], { input: JSON.stringify({ tool_name: toolName, tool_input: {} }), env, encoding: 'utf8' });
}

test('C-memhook.1: 가용 메모리가 기준보다 적으면 문맥 안내 한 줄을 출력하고 실행은 막지 않는다', () => {
    const output = run('Bash', 999999999);
    const parsed = JSON.parse(output);
    assert.equal(parsed.hookSpecificOutput.hookEventName, 'PreToolUse');
    assert.match(parsed.hookSpecificOutput.additionalContext, /^Memory is low: \d+ MB available \(limit 999999999 MB\)/);
    assert.equal(parsed.hookSpecificOutput.permissionDecision, undefined);
});

test('C-memhook.2: 부족한 동안은 메모리를 쓰는 도구(Bash·Agent·Task)를 쓸 때마다 안내한다', () => {
    for (const tool of ['Bash', 'Agent', 'Task']) {
        assert.notEqual(run(tool, 999999999), '', tool);
    }
});

test('C-memhook.3: 충분하면 아무것도 출력하지 않는다', () => {
    assert.equal(run('Bash', 1), '');
    assert.equal(run('Bash', 0), '');
});

test('C-memhook.4: 읽기·편집처럼 메모리를 쓰지 않는 도구에는 부족해도 안내하지 않는다', () => {
    for (const tool of ['Read', 'Edit', 'Grep', 'Write']) {
        assert.equal(run(tool, 999999999), '', tool);
    }
});

test('C-memhook.5: 기준값이 숫자가 아니면 조용히 넘어간다', () => {
    assert.equal(run('Bash', 'abc'), '');
});

test('C-memhook.6: 같은 폴더에 꺼짐 표시 파일이 있으면 부족해도 아무것도 출력하지 않고, 지우면 다시 안내한다', () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'memhook-off-'));
    const script = path.join(directory, 'memory-hook.sh');
    fs.copyFileSync(HOOK, script);
    const runCopy = () => execFileSync('sh', [script], {
        input: JSON.stringify({ tool_name: 'Bash' }),
        env: { ...process.env, PADDOCK_MEMORY_MIN_MB: '999999999' },
        encoding: 'utf8',
    });
    assert.notEqual(runCopy(), '');
    fs.writeFileSync(path.join(directory, 'memory-hook.off'), '');
    assert.equal(runCopy(), '');
    fs.rmSync(path.join(directory, 'memory-hook.off'));
    assert.notEqual(runCopy(), '');
});
