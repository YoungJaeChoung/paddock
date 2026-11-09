// Claude Code 상태 줄(statusline) 명령. Paddock가 사용량 표시를 켜면 Claude Code 실행 인자(`--settings`)의 설정 파일로 등록된다.
// Claude Code가 매 응답 뒤 상태 정보(JSON)를 표준 입력으로 넘기면, 사용량 한도만 파일에 남기고
// 사용자가 원래 쓰던 상태 줄이 있으면 같은 입력으로 불러 그 출력을 그대로 보여 준다. 자격 증명은 읽지 않는다.
// 사용: node claude-statusline.cjs <기본 사용량 저장 파일>
//
// 어느 계정의 사용량인지는 실행 환경의 `CLAUDE_CONFIG_DIR`로 안다.
//   - `.../agent-profiles/<id>/claude` 꼴이면 Paddock 등록 계정이므로 그 폴더의 `paddock-usage/claude.json`에 남긴다.
//   - 아니면(기본 환경) 인자로 받은 기본 사용량 파일에 남긴다.
// 원래 상태 줄은 실행할 때마다 사용자 설정(`$CLAUDE_CONFIG_DIR` 또는 `~/.claude`의 settings.json)의 `statusLine.command`에서 읽는다.
// 그 명령이 이 스크립트를 가리키면(예전 Paddock이 남긴 항목) 자기 자신을 다시 부르지 않도록 건너뛴다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const SCRIPT_NAME = 'claude-statusline.cjs';
const PROFILE_FOLDER = /[\\/]agent-profiles[\\/][0-9a-f-]{36}[\\/]claude$/i;

/** 사용량을 남길 파일. 등록 계정 폴더면 그 안의 기록 파일, 아니면 기본 파일이다. */
function usageFileFor(
    configDir,
    defaultFile,
) {
    let file = defaultFile;
    if (configDir && PROFILE_FOLDER.test(configDir)) file = path.join(configDir, 'paddock-usage', 'claude.json');
    return file;
}

/** 사용자 설정에 적힌 원래 상태 줄 명령. 없거나 이 스크립트를 가리키거나 읽을 수 없으면 null이다. */
function userStatusLineCommand(
    configDir,
) {
    let command = null;
    try {
        const settings = JSON.parse(fs.readFileSync(path.join(configDir || path.join(os.homedir(), '.claude'), 'settings.json'), 'utf8'));
        const line = settings?.statusLine;
        if (line?.type === 'command' && typeof line.command === 'string' && !line.command.includes(SCRIPT_NAME)) command = line.command;
    } catch {
        command = null;
    }
    return command;
}

function run() {
    const configDir = process.env.CLAUDE_CONFIG_DIR;
    const usagePath = usageFileFor(configDir, process.argv[2]);
    const raw = fs.readFileSync(0, 'utf8');
    let input = {};
    try {
        input = JSON.parse(raw);
    } catch {
        input = {};
    }
    if (input.rate_limits && usagePath) {
        // 쓰는 도중에 Paddock가 읽어도 깨진 JSON을 보지 않도록 임시 파일에 쓴 뒤 바꿔 끼운다.
        // 기록을 남기지 못해도(폴더 권한 등) 상태 줄 출력은 그대로 보여 준다.
        try {
            fs.mkdirSync(path.dirname(usagePath), { recursive: true });
            const temporary = `${usagePath}.${process.pid}.tmp`;
            fs.writeFileSync(temporary, JSON.stringify({ rate_limits: input.rate_limits, updated_at: Math.floor(Date.now() / 1000) }));
            fs.renameSync(temporary, usagePath);
        } catch {
            // 다음 응답 뒤에 다시 시도한다.
        }
    }
    const original = userStatusLineCommand(configDir);
    let output = '';
    if (original) {
        const result = spawnSync(original, { shell: true, input: raw, encoding: 'utf8' });
        output = result.stdout || '';
    } else {
        const model = input.model?.display_name || 'Claude';
        const context = input.context_window?.used_percentage;
        output = typeof context === 'number' ? `${model} · ${Math.round(context)}% context` : model;
    }
    process.stdout.write(output);
}

if (require.main === module) run();

module.exports = { usageFileFor, userStatusLineCommand };
