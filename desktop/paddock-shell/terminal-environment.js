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

module.exports = { distributionOf, withDistribution, environmentLabel };
