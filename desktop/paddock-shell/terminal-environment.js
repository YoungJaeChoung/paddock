const { isWslShell } = require('./wsl-terminals');

/** The folder's distribution takes precedence over a launcher's default distribution. */
function distributionOf(
    options = {},
    cwd = '',
) {
    const folder = String(cwd).match(/^(?:file:\/\/|\\\\)(?:wsl\.localhost|wsl\$)[\\/]([^\\/]+)/i);
    const args = Array.isArray(options.shellArgs) ? options.shellArgs : [];
    const index = args.findIndex(argument => argument === '-d' || argument === '--distribution');
    return folder ? decodeURIComponent(folder[1]) : options.paddockAccount?.wslDistribution
        || (index >= 0 ? args[index + 1] : '') || args.find(argument => argument.startsWith('--distribution='))?.slice('--distribution='.length) || '';
}

/** Replaces the distribution selector without changing the remaining shell arguments. */
function withDistribution(
    args,
    distribution,
) {
    const kept = [];
    for (let index = 0; index < args.length; index += 1) {
        if (args[index] === '-d' || args[index] === '--distribution') index += 1;
        else if (!args[index].startsWith('--distribution=')) kept.push(args[index]);
    }
    return ['--distribution', distribution, ...kept];
}

/** Names the execution environment independently of the terminal's foreground program. */
function environmentLabel(
    options = {},
    cwd = '',
    platform = '',
) {
    if (isWslShell(options.shellPath) || options.paddockAccount?.runtime === 'wsl') {
        const distribution = distributionOf(options, cwd);
        return distribution ? `WSL · ${distribution}` : 'WSL';
    }
    const executable = String(options.shellPath || '').split(/[\\/]/).pop().replace(/\.exe$/i, '');
    const names = { pwsh: 'PowerShell', powershell: 'PowerShell', cmd: 'Command Prompt' };
    const shell = names[executable.toLowerCase()] || (executable.toLowerCase() === 'bash' && platform === 'Windows' ? 'Git Bash' : executable);
    return [shell, platform].filter(Boolean).join(' · ') || 'System shell';
}

class InheritedSession {
    /**
     * Paddock을 실행한 Claude Code 세션이 자식 프로세스에 남기는 표지. 앱이 물려받으면 모든 터미널로 번진다.
     * 터미널 안에서 새로 띄운 Claude는 이 표지를 보고 자신을 하위 세션으로 여겨 대화 저장을 끈다.
     * 사용자가 정해 두는 Claude 설정 변수(CLAUDE_CONFIG_DIR, CLAUDE_CODE_USE_BEDROCK 등)는 목록에 없으므로 남는다.
     */
    static NAMES = [
        'CLAUDECODE', 'CLAUDE_CODE_CHILD_SESSION', 'CLAUDE_CODE_SESSION_ID', 'CLAUDE_CODE_BRIDGE_SESSION_ID',
        'CLAUDE_CODE_ENTRYPOINT', 'CLAUDE_CODE_EXECPATH', 'CLAUDE_CODE_MESSAGING_SOCKET', 'CLAUDE_CODE_MESSAGING_TOKEN',
        'CLAUDE_CODE_SESSION_ATTENDED',
    ];
}

/**
 * 앱을 실행한 Claude Code 세션의 표지를 환경에서 지운다. 지운 이름 목록을 돌려준다(없으면 빈 목록).
 *
 * Examples
 * | 실행한 곳                    | 지우는 것                                     |
 * | Claude Code 안의 터미널       | CLAUDECODE, CLAUDE_CODE_CHILD_SESSION, ...    |
 * | 시작 메뉴·바탕화면            | 없음                                          |
 */
function clearInheritedSession(
    env,
) {
    const removed = InheritedSession.NAMES.filter(name => name in env);
    for (const name of removed) delete env[name];
    return removed;
}

module.exports = { distributionOf, withDistribution, environmentLabel, clearInheritedSession, InheritedSession };
