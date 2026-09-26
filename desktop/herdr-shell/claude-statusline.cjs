// Claude Code 상태 줄(statusline) 명령. Herdr가 사용량 표시를 켤 때 Claude Code 설정에 등록한다.
// Claude Code가 매 응답 뒤 상태 정보(JSON)를 표준 입력으로 넘기면, 사용량 한도만 파일에 남기고
// 원래 쓰던 상태 줄이 있으면 같은 입력으로 불러 그 출력을 그대로 보여 준다. 자격 증명은 읽지 않는다.
// 사용: node claude-statusline.cjs <사용량 저장 파일> <원래 상태 줄 기록 파일>
const fs = require('node:fs');
const { spawnSync } = require('node:child_process');

const [usagePath, previousPath] = process.argv.slice(2);
const raw = fs.readFileSync(0, 'utf8');
let input = {};
try {
    input = JSON.parse(raw);
} catch {
    input = {};
}
if (input.rate_limits && usagePath) {
    // 쓰는 도중에 Herdr가 읽어도 깨진 JSON을 보지 않도록 임시 파일에 쓴 뒤 바꿔 끼운다.
    const temporary = `${usagePath}.${process.pid}.tmp`;
    fs.writeFileSync(temporary, JSON.stringify({ rate_limits: input.rate_limits, updated_at: Math.floor(Date.now() / 1000) }));
    fs.renameSync(temporary, usagePath);
}
let previous = null;
try {
    previous = JSON.parse(fs.readFileSync(previousPath, 'utf8'));
} catch {
    previous = null;
}
let output = '';
if (previous?.command) {
    const result = spawnSync(previous.command, { shell: true, input: raw, encoding: 'utf8' });
    output = result.stdout || '';
} else {
    const model = input.model?.display_name || 'Claude';
    const context = input.context_window?.used_percentage;
    output = typeof context === 'number' ? `${model} · ${Math.round(context)}% context` : model;
}
process.stdout.write(output);
