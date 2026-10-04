/** Asks the official CLI for account limits; authentication stays inside Codex. */
const childProcess = require('node:child_process');
const { AccountLaunch } = require('./account-launch');
const { encodePowerShell } = require('./cwd-report');
const { windowLabel } = require('./usage-model');
const { usageError, codexUsage } = require('./usage-files');

class CodexUsage {
    static CACHE_MS = 120000;
    static TIMEOUT_MS = 15000;
    static CLEANUP_MS = 1000;
    static N_OUTPUT_BYTES = 1024 * 1024;

    constructor() {
        this.pending = new Map();
        this.cached = new Map();
    }

    /** Keeps default native and WSL lookups independent, so one unavailable CLI cannot hide another provider. */
    async readAvailable(
        scopes,
    ) {
        const readings = await Promise.all(scopes.map(async scope => {
            let result = { windows: [], updatedAt: null };
            let failed = false;
            try { result = await this.read(scope); } catch { failed = true; }
            if (!result.windows.length) {
                try {
                    const recorded = codexUsage([scope.sessionsDirectory]);
                    if (recorded.windows.length) result = recorded;
                } catch { failed = true; }
            }
            if (!result.windows.length && failed) result.error = 'Codex usage could not be read. Open Codex, sign in if needed, and try again.';
            return result;
        }));
        const available = readings.filter(reading => reading.windows.length);
        let result = available[0] || readings.find(reading => reading.error) || { windows: [], updatedAt: null };
        if (available.length > 1) result = { windows: [], updatedAt: null, error: 'More than one CLI environment has usage. Choose a saved account to see its usage.' };
        return result;
    }

    /** Coalesces requests and keeps successful account-specific readings for two minutes. */
    async read(
        scope,
    ) {
        const key = JSON.stringify([scope.profile.runtime, scope.profile.wslDistribution || '', scope.configDir]);
        const cached = this.cached.get(key);
        let result;
        if (cached && Date.now() - cached.readAt < CodexUsage.CACHE_MS) {
            result = cached.value;
        } else {
            if (!this.pending.has(key)) {
                const pending = readRateLimits(scope).then(value => {
                    this.cached.set(key, { readAt: Date.now(), value });
                    return value;
                }).finally(() => this.pending.delete(key));
                this.pending.set(key, pending);
            }
            result = await this.pending.get(key);
        }
        return result;
    }
}

/** Uses the CLI's default bucket; model-specific buckets are not combined into account totals. */
function rateLimitWindows(
    response,
) {
    const limits = response?.rateLimits;
    const windows = [];
    for (const entry of [limits?.primary, limits?.secondary]) {
        if (entry && Number.isFinite(entry.usedPercent) && Number.isFinite(entry.windowDurationMins) && entry.windowDurationMins > 0) {
            windows.push({ label: windowLabel(entry.windowDurationMins), used: Math.round(Math.max(0, Math.min(100, entry.usedPercent))), ...(Number.isFinite(entry.resetsAt) ? { resetsAt: entry.resetsAt } : {}) });
        }
    }
    return windows;
}

/** Selects the saved environment after shell initialization, without interpolating account paths into shell text. */
function codexUsageCommand(
    scope,
) {
    const env = { ...process.env, CODEX_HOME: scope.configDir };
    for (const name of AccountLaunch.AUTH_ENV) delete env[name];
    const options = { env, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] };
    let command;
    let argumentsList;
    if (scope.profile.runtime === 'wsl' || process.platform !== 'win32') {
        // A WSL launcher cannot signal a Linux child reliably. timeout owns only this lookup's
        // process group and bounds it even if its Windows pipe disappears during shutdown.
        const launcher = scope.profile.runtime === 'wsl' ? 'timeout --signal=TERM --kill-after=2s 14s' : '';
        const script = `unset ${AccountLaunch.AUTH_ENV.join(' ')}; export CODEX_HOME="$1"; cd -- "$CODEX_HOME" || exit 1; exec ${launcher} codex app-server`;
        argumentsList = ['-ic', script, 'paddock-usage', scope.configDir];
        if (scope.profile.runtime === 'wsl') {
            command = 'wsl.exe';
            argumentsList = [...(scope.profile.wslDistribution ? ['--distribution', scope.profile.wslDistribution] : []), '--exec', '/bin/bash', ...argumentsList];
        } else {
            command = '/bin/bash';
            options.cwd = scope.configDir;
            options.detached = true;
        }
    } else {
        command = 'powershell.exe';
        const script = '$ErrorActionPreference = "Stop"; $cli = Get-Command codex -CommandType Application -ErrorAction Stop | Select-Object -First 1; & $cli.Source app-server';
        argumentsList = ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', encodePowerShell(script)];
        options.cwd = scope.configDir;
    }
    return { command, argumentsList, options };
}

/** Exchanges only initialization and usage messages; it never starts a thread or model request. */
function readRateLimits(
    scope,
) {
    const invocation = codexUsageCommand(scope);
    const result = new Promise((resolve, reject) => {
        const child = childProcess.spawn(invocation.command, invocation.argumentsList, invocation.options);
        let buffer = '';
        let n_outputBytes = 0;
        let settled = false;
        let closed = false;
        let initialized = false;
        let cleanupTimer;
        const terminate = () => {
            if (!closed) {
                if (process.platform === 'win32' && scope.profile.runtime === 'native' && child.pid) {
                    // End only the process tree created for this lookup, including npm's wrapper.
                    const cleanup = childProcess.spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
                    cleanup.on('error', () => {});
                } else if (invocation.options.detached && child.pid) {
                    try { process.kill(-child.pid, 'SIGKILL'); } catch { /* The lookup may already have exited. */ }
                } else {
                    child.kill('SIGKILL');
                }
            }
        };
        const finish = (error, value) => {
            if (!settled) {
                settled = true;
                clearTimeout(timer);
                child.stdin.end();
                // EOF normally stops app-server. Reap an unresponsive lookup after a short grace period.
                if (!closed) cleanupTimer = setTimeout(terminate, CodexUsage.CLEANUP_MS);
                if (error) reject(error);
                else resolve(value);
            }
        };
        const unavailable = () => usageError('Codex usage could not be read. Open this account in Codex, sign in if needed, and try again.', 503);
        const timer = setTimeout(() => finish(usageError('Codex usage took too long to respond. Try again.', 503)), CodexUsage.TIMEOUT_MS);
        child.once('error', () => finish(unavailable()));
        child.stdin.on('error', () => finish(unavailable()));
        child.once('close', () => {
            closed = true;
            clearTimeout(cleanupTimer);
            finish(unavailable());
        });
        child.stdout.on('data', chunk => {
            n_outputBytes += chunk.length;
            if (n_outputBytes > CodexUsage.N_OUTPUT_BYTES) finish(unavailable());
            else if (!settled) {
                buffer += chunk.toString('utf8');
                let end;
                while (!settled && (end = buffer.indexOf('\n')) >= 0) {
                    const line = buffer.slice(0, end);
                    buffer = buffer.slice(end + 1);
                    let message;
                    try { message = JSON.parse(line); } catch { message = null; }
                    if (message?.id === 1 && !initialized) {
                        if (message.error || !message.result) finish(unavailable());
                        else {
                            initialized = true;
                            child.stdin.write(`${JSON.stringify({ method: 'initialized', params: {} })}\n`);
                            child.stdin.write(`${JSON.stringify({ method: 'account/rateLimits/read', id: 2 })}\n`);
                        }
                    } else if (message?.id === 2 && initialized) {
                        if (message.error || !message.result) finish(unavailable());
                        else finish(null, { windows: rateLimitWindows(message.result), updatedAt: Math.floor(Date.now() / 1000) });
                    }
                }
            }
        });
        child.stdin.write(`${JSON.stringify({ method: 'initialize', id: 1, params: { clientInfo: { name: 'paddock_usage', title: 'Paddock', version: '0.1.0' } } })}\n`);
    });
    return result;
}

module.exports = { CodexUsage, codexUsageCommand, rateLimitWindows };
