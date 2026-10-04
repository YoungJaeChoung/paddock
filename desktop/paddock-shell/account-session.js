/** Binds a terminal to an exact CLI conversation without reading its messages or login files. */
const fs = require('node:fs');
const path = require('node:path');
const { execFile } = require('node:child_process');
const { AccountProfile } = require('./account-profiles');
const { wslFileMapping } = require('./account-usage');
const { programName } = require('./agent-model');
const { splitCommandLine } = require('./windows-processes');

class SessionIdentity {
    static UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    static TERMINAL = /^[a-zA-Z0-9][a-zA-Z0-9_.:-]{0,199}$/;
    static N_METADATA_BYTES = 64 * 1024;
    static N_PROCESS_DEPTH = 8;
    static STOP_TIMEOUT_MS = 10000;
    static MISSING_FILE = 'The saved conversation file is missing. Restore it before switching accounts; this terminal has not been changed.';
    static WSL_PROCESSES = `for file in $(grep -alz '^PADDOCK_TERMINAL=' /proc/[0-9]*/environ 2>/dev/null); do
    pid=\${file#/proc/}; pid=\${pid%/environ}
    terminal=$({ tr '\\0' '\\n' < "$file"; } 2>/dev/null | sed -n 's/^PADDOCK_TERMINAL=//p')
    stat=$(cat "/proc/$pid/stat" 2>/dev/null) || continue
    set -- \${stat##*) }
    parent=$2
    shift 19
    started=$1
    argv=$({ tr '\\0' '\\037' < "/proc/$pid/cmdline"; } 2>/dev/null)
    printf '%s\\t%s\\t%s\\t%s\\t%s\\n' "$terminal" "$pid" "$parent" "$started" "$argv"
done`;
    static WSL_STOP = `set -eu
pid=$1
expected=$2
stat=$(cat "/proc/$pid/stat" 2>/dev/null) || exit 0
set -- \${stat##*) }
shift 19
[ "$1" = "$expected" ] || exit 3
kill -TERM "$pid"`;
}

function checkedRequest(
    request,
) {
    if (!request || !SessionIdentity.TERMINAL.test(request.terminalId || '')) throw new Error('The terminal identifier is invalid.');
    if (!Number.isSafeInteger(request.shellPid) || request.shellPid <= 0) throw new Error('The terminal process identifier is invalid.');
    if (!['claude', 'codex'].includes(request.provider)) throw new Error('Choose a Claude or Codex terminal.');
    if (!['native', 'wsl'].includes(request.runtime)) throw new Error('The terminal environment is invalid.');
    if (request.accountId !== undefined && request.accountId !== null && !AccountProfile.ID.test(request.accountId)) throw new Error('The source account identifier is invalid.');
    if (request.runtime === 'wsl' && (typeof request.wslDistribution !== 'string' || !request.wslDistribution.trim() || /[\u0000-\u001f\u007f]/.test(request.wslDistribution))) throw new Error('Choose the terminal’s WSL distribution.');
    if (request.resume && (request.resume.provider !== request.provider || request.resume.runtime !== request.runtime
        || (request.runtime === 'wsl' && request.resume.wslDistribution?.toLowerCase() !== request.wslDistribution.toLowerCase()))) throw new Error('The saved conversation belongs to another CLI or environment.');
    return request;
}

/** Missing paths mean that the CLI has not saved a conversation yet; links are never followed. */
function plainEntry(
    file,
    kind,
) {
    let entry = null;
    try {
        entry = fs.lstatSync(file);
        if (entry.isSymbolicLink() || (kind === 'directory' ? !entry.isDirectory() : !entry.isFile())) throw new Error('A conversation path is redirected or has the wrong file type. Restore its folder before switching accounts.');
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    return entry;
}

/** Only the official CLI's small process registry is read; conversation JSONL contents stay untouched. */
function readClaudeSession(
    scope,
    processIdentity,
) {
    const directory = scope.join(scope.configDir, 'sessions');
    plainEntry(scope.toLocal(directory), 'directory');
    const file = scope.toLocal(scope.join(directory, `${processIdentity.pid}.json`));
    const entry = plainEntry(file, 'file');
    let result = null;
    if (entry && entry.size <= SessionIdentity.N_METADATA_BYTES) {
        const record = JSON.parse(fs.readFileSync(file, 'utf8'));
        const started = record.procStartFt ?? record.procStart;
        if (record.pid === processIdentity.pid && typeof started === 'string' && started === processIdentity.start
            && record.kind === 'interactive' && SessionIdentity.UUID.test(record.sessionId || '')
            && typeof record.cwd === 'string' && record.cwd && !/[\u0000-\u001f\u007f]/.test(record.cwd)) {
            result = { sessionId: record.sessionId, cwd: record.cwd };
        }
    }
    return result;
}

/** Exact UUID filenames identify the saved conversation; modification times never choose a conversation. */
function findClaudeTranscript(
    scope,
    sessionId,
) {
    if (!SessionIdentity.UUID.test(sessionId || '')) throw new Error('The conversation identifier is invalid.');
    const directory = scope.join(scope.configDir, 'projects');
    const matches = [];
    if (plainEntry(scope.toLocal(directory), 'directory')) {
        for (const entry of fs.readdirSync(scope.toLocal(directory), { withFileTypes: true })) {
            if (entry.isDirectory()) {
                const project = scope.join(directory, entry.name);
                plainEntry(scope.toLocal(project), 'directory');
                const transcript = scope.join(project, `${sessionId}.jsonl`);
                if (plainEntry(scope.toLocal(transcript), 'file')) matches.push(transcript);
            }
        }
    }
    if (matches.length > 1) throw new Error('The conversation was found in more than one project folder. Choose it in the official CLI before switching accounts.');
    return matches[0] || null;
}

/** Rechecks a path previously resolved by this backend, including the project directory above it. */
function existingTranscript(
    scope,
    transcriptPath,
) {
    plainEntry(scope.toLocal(scope.join(scope.configDir, 'projects')), 'directory');
    plainEntry(path.dirname(scope.toLocal(transcriptPath)), 'directory');
    const exists = plainEntry(scope.toLocal(transcriptPath), 'file');
    return exists ? transcriptPath : null;
}

/** Stops at the first agent in each branch so its own subagents cannot be mistaken for the terminal's conversation. */
function chooseAgentProcess(
    processes,
    shellPid,
    provider,
) {
    let level = processes.filter(item => item.pid === shellPid);
    const matches = [];
    const seen = new Set();
    for (let n_depth = 0; level.length && n_depth < SessionIdentity.N_PROCESS_DEPTH; n_depth += 1) {
        const parents = [];
        for (const item of level) {
            if (!seen.has(item.pid)) {
                seen.add(item.pid);
                if (programName(item.argv) === provider) matches.push(item);
                else parents.push(item.pid);
            }
        }
        level = processes.filter(item => parents.includes(item.parentPid) && !seen.has(item.pid));
    }
    return matches.length === 1 ? matches[0] : null;
}

function executeFile(
    command,
    argumentsList,
) {
    const result = new Promise((resolve, reject) => {
        execFile(command, argumentsList, { encoding: 'utf8', timeout: 15000, maxBuffer: 4 * 1024 * 1024, windowsHide: true,
            env: { ...process.env, LC_ALL: 'C', TZ: 'UTC' } }, (error, output) => {
            if (error) reject(new Error('The terminal process could not be inspected. Keep its CLI open and try again.'));
            else resolve(output);
        });
    });
    return result;
}

function linuxProcess(
    pid,
) {
    let result = null;
    try {
        const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
        const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
        if (fields[0] !== 'Z') result = { pid, parentPid: Number(fields[1]), start: fields[19], argv: fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').split('\0').filter(Boolean) };
    } catch (error) {
        if (!['ENOENT', 'ESRCH', 'EACCES'].includes(error.code)) throw error;
    }
    return result;
}

class AccountSessions {
    constructor(
        { accounts, readWslInfo, readWindowsProcesses },
    ) {
        this.accounts = accounts;
        this.readWslInfo = readWslInfo;
        this.readWindowsProcesses = readWindowsProcesses;
        this.observed = new Map();
        this.prepared = new Map();
        this.seen = new Set();
        this.activeObserved = new Set();
        this.stopping = new Set();
        this.stopped = new Map();
        this.unavailable = new Map();
    }

    /** One WSL process read serves every terminal in that distribution in the observation batch. */
    async observeSessions(
        requests,
    ) {
        if (!Array.isArray(requests) || requests.length > 200) throw new Error('The terminal observation list is invalid.');
        const processReads = new Map();
        const result = {};
        await Promise.all(requests.map(async request => {
            try {
                checkedRequest(request);
                const snapshot = await this.observe(request, processReads);
                if (snapshot !== undefined) result[request.terminalId] = snapshot;
            } catch {
                // One removed account or temporarily unreadable registry must not prevent
                // other terminals from keeping their own conversation bindings current.
            }
        }));
        return result;
    }

    async observeSession(
        request,
    ) {
        checkedRequest(request);
        const snapshot = await this.observe(request, new Map());
        return snapshot;
    }

    async scope(
        request,
        accountId = request.accountId || null,
    ) {
        const profile = accountId ? (await this.accounts.list()).find(item => item.id === accountId) : null;
        if (accountId && !profile) throw new Error('The conversation’s source account was removed. Restore its account entry before switching.');
        if (profile && (profile.provider !== request.provider || profile.runtime !== request.runtime
            || (profile.runtime === 'wsl' && profile.wslDistribution.toLowerCase() !== request.wslDistribution.toLowerCase()))) throw new Error('The source account belongs to another CLI or environment.');
        let configDir;
        let toLocal = file => file;
        const join = request.runtime === 'wsl' ? path.posix.join : path.join;
        if (request.runtime === 'wsl') {
            if (process.platform !== 'win32') throw new Error('Open this WSL terminal on its Windows device.');
            const mapping = wslFileMapping(await this.readWslInfo(request.wslDistribution), request.wslDistribution);
            toLocal = mapping.toLocal;
            configDir = profile ? join(mapping.linuxHome, '.paddock', 'agent-profiles', profile.id, profile.provider) : join(mapping.linuxHome, '.claude');
            if (profile) {
                for (const directory of [join(mapping.linuxHome, '.paddock'), join(mapping.linuxHome, '.paddock', 'agent-profiles'), join(mapping.linuxHome, '.paddock', 'agent-profiles', profile.id)]) plainEntry(toLocal(directory), 'directory');
            }
        } else {
            configDir = profile ? join(this.accounts.configDirectory, 'agent-profiles', profile.id, profile.provider) : this.accounts.claudeDirectory;
            if (profile) {
                for (const directory of [this.accounts.configDirectory, join(this.accounts.configDirectory, 'agent-profiles'), join(this.accounts.configDirectory, 'agent-profiles', profile.id)]) plainEntry(directory, 'directory');
            }
        }
        plainEntry(toLocal(configDir), 'directory');
        return { profile, configDir, toLocal, join };
    }

    async processes(
        request,
        reads,
    ) {
        const key = request.runtime === 'wsl' ? `wsl:${request.wslDistribution.toLowerCase()}` : `native:${process.platform === 'linux' ? request.shellPid : 'all'}`;
        if (!reads.has(key)) {
            reads.set(key, Promise.resolve().then(async () => {
                let processes = [];
                if (request.runtime === 'wsl') {
                    const output = await executeFile('wsl.exe', ['--distribution', request.wslDistribution, '--exec', 'sh', '-c', SessionIdentity.WSL_PROCESSES]);
                    processes = output.split('\n').filter(Boolean).map(line => {
                        const [terminalId, pid, parentPid, start, argv = ''] = line.replace(/\r$/, '').split('\t');
                        return { terminalId, pid: Number(pid), parentPid: Number(parentPid), start, argv: argv.split('\x1f').filter(Boolean) };
                    });
                } else if (process.platform === 'win32') {
                    processes = (await this.readWindowsProcesses()).map(item => ({ ...item, start: item.startIdentity, argv: splitCommandLine(item.commandLine) }));
                } else if (process.platform === 'darwin') {
                    const output = await executeFile('/bin/ps', ['-axo', 'pid=,ppid=,lstart=,command=']);
                    processes = output.split('\n').flatMap(line => {
                        const match = /^\s*(\d+)\s+(\d+)\s+(.{24})\s+(.*)$/.exec(line);
                        return match ? [{ pid: Number(match[1]), parentPid: Number(match[2]), start: match[3].trim(), argv: splitCommandLine(match[4]) }] : [];
                    });
                } else if (process.platform === 'linux') {
                    let pids = [request.shellPid];
                    for (let n_depth = 0; pids.length && n_depth < SessionIdentity.N_PROCESS_DEPTH; n_depth += 1) {
                        const children = [];
                        for (const pid of pids) {
                            const item = linuxProcess(pid);
                            if (item) {
                                processes.push(item);
                                try {
                                    children.push(...fs.readFileSync(`/proc/${pid}/task/${pid}/children`, 'utf8').trim().split(/\s+/).filter(Boolean).map(Number));
                                } catch (error) {
                                    if (error.code !== 'ENOENT') throw error;
                                }
                            }
                        }
                        pids = children;
                    }
                }
                return processes;
            }));
        }
        const processes = await reads.get(key);
        return request.runtime === 'wsl' ? processes.filter(item => item.terminalId === request.terminalId) : processes;
    }

    frame(
        request,
        processes,
    ) {
        const shell = request.runtime === 'wsl'
            ? processes.find(item => !processes.some(parent => parent.pid === item.parentPid))
            : processes.find(item => item.pid === request.shellPid);
        const selected = shell && chooseAgentProcess(processes, shell.pid, request.provider);
        const descendants = new Set(shell ? [shell.pid] : []);
        for (let n_depth = 0; n_depth < SessionIdentity.N_PROCESS_DEPTH; n_depth += 1) {
            for (const item of processes) if (descendants.has(item.parentPid)) descendants.add(item.pid);
        }
        const busy = processes.some(item => item.pid !== shell?.pid && descendants.has(item.pid)
            && !['conhost', 'openconsole'].includes(programName(item.argv)));
        return { shell, selected, busy };
    }

    async observe(
        request,
        reads,
    ) {
        const key = `${request.terminalId}:${request.shellPid}`;
        const wasObserved = this.seen.has(key);
        this.seen.add(key);
        this.unavailable.delete(key);
        let result = null;
        if (request.provider === 'claude') {
            const scope = await this.scope(request);
            const frame = this.frame(request, await this.processes(request, reads));
            let previous = this.observed.get(key);
            const stopped = this.stopped.get(key);
            if (previous && ((frame.shell && previous.shellStart !== frame.shell.start) || (!frame.shell && !stopped))) {
                this.observed.delete(key);
                this.stopped.delete(key);
                previous = null;
            }
            if (this.stopping.has(key)) result = previous?.snapshot || null;
            else if (frame.shell && frame.selected) {
                this.activeObserved.add(key);
                this.stopped.delete(key);
                const record = readClaudeSession(scope, frame.selected);
                if (record) {
                    const retained = previous?.snapshot?.sessionId === record.sessionId ? previous.snapshot
                        : request.resume?.sessionId === record.sessionId ? request.resume : null;
                    const sourceAccountId = retained ? retained.sourceAccountId || null : request.accountId || null;
                    const origin = await this.scope(request, sourceAccountId);
                    let transcriptPath = null;
                    if (retained === previous?.snapshot) {
                        transcriptPath = existingTranscript(origin, retained.transcriptPath);
                    } else {
                        transcriptPath = findClaudeTranscript(origin, record.sessionId);
                    }
                    if (transcriptPath) {
                        result = { provider: 'claude', sessionId: record.sessionId, sourceAccountId, runtime: request.runtime,
                            shellPid: request.shellPid, shellStart: frame.shell.start,
                            ...(request.runtime === 'wsl' ? { wslDistribution: request.wslDistribution } : {}), cwd: record.cwd, transcriptPath };
                        this.observed.set(key, { snapshot: result, shellStart: frame.shell.start, agentPid: frame.selected.pid, agentStart: frame.selected.start });
                    } else if (retained) {
                        // Losing a known file must never turn its saved --resume launcher
                        // into a fresh conversation when the workspace is restored.
                        this.unavailable.set(key, SessionIdentity.MISSING_FILE);
                        result = undefined;
                    }
                }
                if (!result) this.observed.delete(key);
                // A CLI can appear before its registry. Omit this terminal from the response
                // until it reports an identity, preserving the origin path during resume startup.
                if (!record) result = undefined;
            } else if (frame.shell && !frame.busy) {
                result = previous?.snapshot || null;
                if (result && !existingTranscript(await this.scope(request, result.sourceAccountId), result.transcriptPath)) {
                    this.unavailable.set(key, SessionIdentity.MISSING_FILE);
                    result = undefined;
                }
                // Restoration accepts an exact identifier and recalculates its path on the server.
                // A frontend-supplied transcript path is never used as a filesystem input.
                if (!result && !wasObserved && request.resume?.provider === 'claude' && SessionIdentity.UUID.test(request.resume.sessionId || '')
                    && request.resume.shellPid === request.shellPid && request.resume.shellStart === frame.shell.start) {
                    const origin = await this.scope(request, request.resume.sourceAccountId || null);
                    const transcriptPath = findClaudeTranscript(origin, request.resume.sessionId);
                    if (transcriptPath && typeof request.resume.cwd === 'string' && request.resume.cwd && !/[\u0000-\u001f\u007f]/.test(request.resume.cwd)) {
                        result = { provider: 'claude', sessionId: request.resume.sessionId, sourceAccountId: request.resume.sourceAccountId || null,
                            shellPid: request.shellPid, shellStart: frame.shell.start,
                            runtime: request.runtime, ...(request.runtime === 'wsl' ? { wslDistribution: request.wslDistribution } : {}), cwd: request.resume.cwd, transcriptPath };
                        this.observed.set(key, { snapshot: result, shellStart: frame.shell.start, agentPid: null, agentStart: null });
                    } else if (!transcriptPath) {
                        this.unavailable.set(key, SessionIdentity.MISSING_FILE);
                        result = undefined;
                    }
                }
                if (!result && request.resume && !this.activeObserved.has(key)
                    && (request.resume.shellPid !== request.shellPid || request.resume.shellStart !== frame.shell.start)) {
                    // A replacement shell may still be loading the user's startup file.
                    // Preserve the requested origin while refusing to claim that resume succeeded.
                    result = undefined;
                }
            } else if (frame.shell && frame.busy) {
                this.unavailable.set(key, 'Finish the other command in this terminal before switching accounts.');
                result = undefined;
            } else if (!frame.shell && stopped) {
                // A failed PTY replacement may already have closed the original shell. Only a
                // conversation whose main CLI was stopped by this backend can retry without it.
                result = stopped.snapshot;
                if (!existingTranscript(await this.scope(request, result.sourceAccountId), result.transcriptPath)) {
                    this.unavailable.set(key, SessionIdentity.MISSING_FILE);
                    result = undefined;
                }
            }
        }
        return result;
    }

    /** Prepares another account only after the source conversation and environment have been verified. */
    async prepareResume(
        targetId,
        request,
    ) {
        checkedRequest(request);
        if (request.provider === 'codex') throw new Error('Codex account switching in the same conversation is not available yet. Open the other account in a new terminal.');
        const target = (await this.accounts.list()).find(item => item.id === targetId);
        if (!target) throw new Error('The selected account is no longer available. Refresh the account list.');
        if (target.id === request.accountId) throw new Error('This terminal already uses that account.');
        if (target.provider !== request.provider || target.runtime !== request.runtime
            || (target.runtime === 'wsl' && target.wslDistribution.toLowerCase() !== request.wslDistribution.toLowerCase())) throw new Error('Continue with an account for the same CLI and environment.');
        const resume = await this.observeSession(request);
        const key = `${request.terminalId}:${request.shellPid}`;
        if (!resume) throw new Error(this.unavailable.get(key) || 'This terminal’s conversation could not be identified. Keep Claude open until its conversation is saved, then try again.');
        const observed = this.observed.get(key);
        const prepared = await this.accounts.prepare(targetId);
        this.prepared.set(key, observed);
        return { ...prepared, resume };
    }

    /** Signals only the verified main CLI and waits for its exit; other commands are never terminated. */
    async stopSession(
        request,
    ) {
        checkedRequest(request);
        const key = `${request.terminalId}:${request.shellPid}`;
        const before = this.prepared.get(key);
        if (!before) throw new Error('Prepare the account change before stopping its conversation.');
        if (this.observed.get(key)?.snapshot.sessionId !== before.snapshot.sessionId) throw new Error('The conversation changed. Open the account menu again.');
        if (!existingTranscript(await this.scope(request, before.snapshot.sourceAccountId), before.snapshot.transcriptPath)) throw new Error(SessionIdentity.MISSING_FILE);
        const frame = this.frame(request, await this.processes(request, new Map()));
        const stopped = this.stopped.get(key);
        if ((!frame.shell && (!stopped || stopped.snapshot.sessionId !== before.snapshot.sessionId))
            || (frame.shell && frame.shell.start !== before.shellStart)) throw new Error('The terminal process changed. Open the account menu again.');
        if (!frame.selected && frame.busy) throw new Error('Another command is running in this terminal. Finish it before switching accounts.');
        if (frame.selected) {
            const scope = await this.scope(request);
            const record = readClaudeSession(scope, frame.selected);
            if (frame.selected.pid !== before.agentPid || frame.selected.start !== before.agentStart || record?.sessionId !== before.snapshot.sessionId) throw new Error('The conversation changed. Open the account menu again.');
            if (request.runtime !== 'wsl' && process.platform === 'win32') throw new Error('Exit Claude with Ctrl+C first, then switch accounts. Windows cannot safely stop this CLI automatically.');
            this.stopping.add(key);
            try {
                if (request.runtime === 'wsl') {
                    await executeFile('wsl.exe', ['--distribution', request.wslDistribution, '--exec', 'sh', '-c', SessionIdentity.WSL_STOP, 'paddock-stop-session', String(frame.selected.pid), frame.selected.start]);
                } else {
                    const current = process.platform === 'linux' ? linuxProcess(frame.selected.pid) : (await this.processes(request, new Map())).find(item => item.pid === frame.selected.pid);
                    if (current?.start !== frame.selected.start) throw new Error('The CLI process changed before it could stop. Try again.');
                    process.kill(frame.selected.pid, 'SIGTERM');
                }
                const deadline = Date.now() + SessionIdentity.STOP_TIMEOUT_MS;
                let running = true;
                while (running && Date.now() < deadline) {
                    await new Promise(resolve => setTimeout(resolve, 150));
                    const current = await this.processes(request, new Map());
                    running = current.some(item => item.pid === frame.selected.pid && item.start === frame.selected.start);
                }
                if (running) throw new Error('Claude did not stop in time. Exit it in the terminal before switching accounts.');
            } finally {
                this.stopping.delete(key);
            }
        }
        this.stopped.set(key, before);
        return { stopped: true, resume: before.snapshot };
    }
}

module.exports = { AccountSessions, findClaudeTranscript, readClaudeSession, chooseAgentProcess };
