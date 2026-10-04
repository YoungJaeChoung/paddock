const { encodePowerShell, cwdReportOptions } = require('./cwd-report');
const { BrowserBridge } = require('./account-browser');

class AccountLaunch {
    static PROVIDERS = { claude: 'Claude', codex: 'Codex' };
    // These selectors can bypass the CLI login in the chosen configuration directory.
    // Only their names are used; no credential value is inspected or put in a command.
    static AUTH_ENV = [
        'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_PROFILE',
        'CLAUDE_CODE_OAUTH_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR',
        'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY',
        'OPENAI_API_KEY', 'CODEX_API_KEY', 'OPENAI_BASE_URL', 'CODEX_AUTH_JSON',
    ];
}

function quotePosix(
    value,
) {
    return `'${String(value).replace(/'/g, `'"'"'`)}'`;
}

function quotePowerShell(
    value,
) {
    return `'${String(value).replace(/'/g, "''")}'`;
}

/** Builds a Bash startup file in memory, keeping the account environment after the CLI exits. */
function bashArguments(
    provider,
    configDir,
    browserDirectory,
) {
    const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
    // Bash reads the user's aliases and PATH before applying the selected account.
    // A pipe-backed rcfile avoids writing launch scripts alongside CLI-owned credentials.
    const script = [
        'if [ -f ~/.bashrc ]; then . ~/.bashrc; fi',
        `unset ${AccountLaunch.AUTH_ENV.join(' ')}`,
        `export ${variable}=${quotePosix(configDir)}`,
        // WSL uses the Windows browser while the official CLI owns the entire login flow.
        ...(browserDirectory ? [`export PATH=${quotePosix(browserDirectory)}:"$PATH"`, `export BROWSER=${quotePosix(BrowserBridge.NAME)}`] : []),
        `printf '\\n%s\\n' ${quotePosix(`${AccountLaunch.PROVIDERS[provider]} account environment. Sign in through the official CLI if prompted.`)}`,
        // Job control is ready at the first prompt. Running the CLI inside the rcfile itself
        // keeps Bash as the foreground group, hiding the agent from the work/activity detector.
        '__paddock_account_prompt=("${PROMPT_COMMAND[@]}")',
        '__paddock_start_account() {',
        'PROMPT_COMMAND=("${__paddock_account_prompt[@]}")',
        'unset __paddock_account_prompt',
        'unset -f __paddock_start_account',
        `if command -v ${provider} >/dev/null 2>&1; then command ${provider}; else printf '%s\\n' ${quotePosix(`${AccountLaunch.PROVIDERS[provider]} CLI was not found. Install it in this environment, then run ${provider}.`)}; fi`,
        '}',
        'PROMPT_COMMAND=__paddock_start_account',
    ].join('\n');
    return ['-l', '-c', `exec /bin/bash --rcfile <(printf '%s\\n' ${quotePosix(script)}) -i`];
}

/** Native Windows starts a persistent PowerShell; application lookup skips aliases and functions. */
function powerShellArguments(
    provider,
    configDir,
    shellPath,
) {
    const variable = provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
    const report = cwdReportOptions(shellPath);
    const encoded = report.shellArgs.at(-1);
    const bytes = atob(encoded);
    let cwdScript = '';
    for (let offset = 0; offset < bytes.length; offset += 2) cwdScript += String.fromCharCode(bytes.charCodeAt(offset) | (bytes.charCodeAt(offset + 1) << 8));
    const script = [
        cwdScript,
        ...AccountLaunch.AUTH_ENV.map(name => `Remove-Item Env:${name} -ErrorAction SilentlyContinue`),
        `$env:${variable} = ${quotePowerShell(configDir)}`,
        `Write-Host ${quotePowerShell(`${AccountLaunch.PROVIDERS[provider]} account environment. Sign in through the official CLI if prompted.`)}`,
        `$paddockCli = Get-Command ${provider} -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1`,
        `if ($paddockCli) { & $paddockCli.Source } else { Write-Host ${quotePowerShell(`${AccountLaunch.PROVIDERS[provider]} CLI was not found. Install it in this environment, then run ${provider}.`)} }`,
    ].join('; ');
    return ['-NoLogo', '-NoExit', '-EncodedCommand', encodePowerShell(script)];
}

/**
 * Converts a prepared account into restorable terminal options, containing no login information.
 * Workspace paths and configuration paths are arguments or quoted literals, never executable input.
 */
function accountTerminalOptions(
    prepared,
    { cwd, isWindows, wslEnv = '' },
) {
    const { profile, configDir, browserDirectory } = prepared;
    if (!Object.hasOwn(AccountLaunch.PROVIDERS, profile.provider)) throw new Error('Choose a Claude or Codex account.');
    if (typeof configDir !== 'string' || !configDir || /[\u0000-\u001f\u007f]/.test(configDir)) throw new Error('The account configuration path is invalid.');
    if (browserDirectory !== undefined && (typeof browserDirectory !== 'string' || !browserDirectory.startsWith('/') || /[\u0000-\u001f\u007f]/.test(browserDirectory))) throw new Error('The account browser path is invalid.');
    const variable = profile.provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
    const env = Object.fromEntries(AccountLaunch.AUTH_ENV.map(name => [name, null]));
    env[variable] = configDir;
    // Noninteractive shell startup hooks must not intercept the initial launcher.
    env.BASH_ENV = null;
    env.ENV = null;
    const options = {
        cwd, title: `${AccountLaunch.PROVIDERS[profile.provider]} · ${profile.label}`,
        paddockAccount: { ...profile }, env,
    };
    if (profile.runtime === 'wsl') {
        if (!isWindows || !profile.wslDistribution) throw new Error('Open this WSL account on its Windows device.');
        options.shellPath = 'C:\\Windows\\System32\\wsl.exe';
        options.shellArgs = ['-d', profile.wslDistribution, '--cd', cwd, '-e', '/bin/bash', ...bashArguments(profile.provider, configDir, browserDirectory)];
        // Keep existing WSL forwarding rules; Linux startup clears auth selectors and reapplies the chosen account.
        options.env = { ...env, WSLENV: wslEnv };
    } else if (isWindows) {
        options.shellPath = 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe';
        options.shellArgs = powerShellArguments(profile.provider, configDir, options.shellPath);
    } else {
        options.shellPath = '/bin/bash';
        options.shellArgs = bashArguments(profile.provider, configDir, browserDirectory);
    }
    return options;
}

module.exports = { AccountLaunch, accountTerminalOptions, quotePosix };
