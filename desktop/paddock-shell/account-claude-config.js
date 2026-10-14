/**
 * Keeps a Claude account profile's `.claude.json` in step with the default environment's `.claude.json`.
 *
 * That file holds user-scope MCP servers and per-folder trust, but also sign-in state, so it cannot be linked or replaced:
 * a profile signs in to its own account. Only two keys are merged, and every other key is left exactly as the CLI wrote it.
 *   - `mcpServers`: the default's servers are copied into the profile (same name: the default wins), and servers Paddock copied earlier
 *     disappear from the profile when the default no longer has them, provided the profile never changed them. A server the profile added
 *     itself stays and is never copied back. Pencil is Paddock's own registration (a fixed path per host), set last and kept out of the copy record.
 *   - `projects`: folders the profile has never seen get the default's trust answers and allowed tools, so a new account does not re-ask
 *     for every folder. Existing folders are never touched.
 * The copy record is `paddock-sync.json` in the profile folder. The WSL preparation script runs the same algorithm in Python (`ClaudeConfig.PYTHON`).
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { pencilEntry } = require('./account-pencil');

class ClaudeConfig {
    // Servers Paddock writes itself; they are neither copied from the default nor recorded as copies.
    static MANAGED_SERVERS = Object.freeze(['pencil']);
    // The per-folder answers worth carrying to a new account. Everything else under `projects.<folder>` is history of that account's sessions.
    static PROJECT_KEYS = Object.freeze([
        'hasTrustDialogAccepted', 'allowedTools', 'mcpServers', 'enabledMcpjsonServers', 'disabledMcpjsonServers',
        'hasClaudeMdExternalIncludesApproved', 'hasClaudeMdExternalIncludesWarningShown',
    ]);
    static SYNC_FILE = 'paddock-sync.json';
    static SYNC_VERSION = 1;
}

function isPlainObject(
    value,
) {
    return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** Whether two JSON values are equal regardless of key order. */
function sameJson(
    left,
    right,
) {
    let isSame = false;
    if (isPlainObject(left) && isPlainObject(right)) {
        const keys = Object.keys(left);
        isSame = keys.length === Object.keys(right).length && keys.every(key => Object.hasOwn(right, key) && sameJson(left[key], right[key]));
    } else if (Array.isArray(left) && Array.isArray(right)) {
        isSame = left.length === right.length && left.every((value, index) => sameJson(value, right[index]));
    } else {
        isSame = left === right;
    }
    return isSame;
}

/**
 * Computes the profile's next `.claude.json` content and the next copy record. Pure: it reads and writes no files.
 *
 * Parameters
 * ----------
 * defaultData : parsed default `.claude.json`, or null when the default is absent (then nothing is copied or removed).
 * profileData : parsed profile `.claude.json`, or null when the profile has none yet.
 * synced : `mcpServers` entries Paddock copied before (from the copy record).
 * pencil : the Pencil server entry to register, or null to leave Pencil as it is.
 *
 * Returns `{ data, synced, warnings }`; `data` is a new object (inputs are not changed) and carries only keys the profile already had
 * plus `mcpServers`/`projects` when something was added. `warnings` is non-empty (and `data` null) when the profile's `mcpServers` or `projects` has an unexpected shape.
 *
 * Examples
 * --------
 * | default servers | record | profile servers      | result servers                       |
 * | --------------- | ------ | -------------------- | ------------------------------------ |
 * | a               | -      | -                    | a (copied, recorded)                 |
 * | a=1             | -      | a=2                  | a=1 (the default wins)               |
 * | -               | a=1    | a=1                  | - (removed: copied, never changed)   |
 * | -               | a=1    | a=2                  | a=2 (changed in the profile: kept)   |
 * | -               | -      | b                    | b (the profile's own)                |
 */
function mergeClaudeConfig(
    { defaultData, profileData, synced, pencil },
) {
    const managed = new Set(ClaudeConfig.MANAGED_SERVERS);
    const data = profileData === null ? {} : structuredClone(profileData);
    const warnings = [];
    const hasShape = (key) => data[key] === undefined || isPlainObject(data[key]);
    if (!hasShape('mcpServers') || !hasShape('projects')) {
        warnings.push('The account sign-in file has an unexpected mcpServers or projects format, so MCP servers were not synced.');
    }
    let nextSynced = { ...synced };
    if (!warnings.length) {
        const servers = { ...(data.mcpServers || {}) };
        const defaultServers = isPlainObject(defaultData?.mcpServers) ? defaultData.mcpServers : {};
        nextSynced = {};
        if (defaultData !== null) {
            for (const [name, value] of Object.entries(defaultServers)) {
                if (!managed.has(name)) {
                    servers[name] = structuredClone(value);
                    nextSynced[name] = structuredClone(value);
                }
            }
            for (const [name, value] of Object.entries(synced)) {
                if (!managed.has(name) && !Object.hasOwn(defaultServers, name) && Object.hasOwn(servers, name) && sameJson(servers[name], value)) delete servers[name];
            }
        } else {
            nextSynced = { ...synced };
        }
        if (pencil) servers[ClaudeConfig.MANAGED_SERVERS[0]] = structuredClone(pencil);
        if (Object.keys(servers).length || data.mcpServers !== undefined) data.mcpServers = servers;
        if (isPlainObject(defaultData?.projects)) {
            const projects = { ...(data.projects || {}) };
            for (const [folder, answers] of Object.entries(defaultData.projects)) {
                if (!Object.hasOwn(projects, folder) && isPlainObject(answers)) {
                    const picked = {};
                    for (const key of ClaudeConfig.PROJECT_KEYS) if (Object.hasOwn(answers, key)) picked[key] = structuredClone(answers[key]);
                    if (Object.keys(picked).length) projects[folder] = picked;
                }
            }
            if (Object.keys(projects).length || data.projects !== undefined) data.projects = projects;
        }
    }
    return { data: warnings.length ? null : data, synced: nextSynced, warnings };
}

/** Parsed JSON object from a file, `null` when the file is absent, or `undefined` when it exists but is not a JSON object. */
function readObject(
    file,
) {
    let result = null;
    let text = null;
    try {
        text = fs.readFileSync(file, 'utf8');
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    if (text !== null) {
        try {
            const parsed = JSON.parse(text);
            result = isPlainObject(parsed) ? parsed : undefined;
        } catch {
            result = undefined;
        }
    }
    return { value: result, text };
}

function writeAtomically(
    file,
    text,
    mode,
) {
    const temporaryFile = path.join(path.dirname(file), `.${path.basename(file)}.${randomUUID()}.tmp`);
    try {
        fs.writeFileSync(temporaryFile, text, { mode, flag: 'wx' });
        fs.renameSync(temporaryFile, file);
    } finally {
        fs.rmSync(temporaryFile, { force: true });
    }
}

/**
 * Syncs the profile's `.claude.json` with the default's (see the file comment) and writes the profile file and the copy record when they change.
 * Returns `{ status, warnings }`; `status` is 'created' (the profile had no file), 'updated', 'unchanged', or 'skipped'.
 *
 * A default that is absent copies nothing and removes nothing. A default or profile file that cannot be parsed makes the whole sync
 * 'skipped' with a warning: the profile file also holds the sign-in, so it is never replaced, and removals computed from an unreadable
 * default would be wrong. The profile file's indentation, trailing newline, and permissions are kept; a new file is created with mode 0600.
 *
 * Examples
 * --------
 * | default file | profile file | status                                      |
 * | ------------ | ------------ | ------------------------------------------- |
 * | has servers  | absent       | created (`mcpServers` and `projects` only) |
 * | has servers  | has them     | unchanged                                   |
 * | has servers  | lacks one    | updated (other keys untouched)              |
 * | broken JSON  | any          | skipped + warning                           |
 * | absent       | absent       | unchanged (nothing to write without Pencil) |
 */
function syncClaudeConfig(
    { defaultFile, profileFile, syncFile, pencilCommand },
) {
    const warnings = [];
    let status = 'skipped';
    const isSameFile = path.resolve(defaultFile) === path.resolve(profileFile);
    const defaults = isSameFile ? { value: undefined } : readObject(defaultFile);
    const profile = readObject(profileFile);
    if (isSameFile) {
        warnings.push('The default and account sign-in files are the same file, so MCP servers were not synced.');
    } else if (defaults.value === undefined) {
        warnings.push('The default Claude sign-in file (.claude.json) could not be read as JSON, so MCP servers were not synced.');
    } else if (profile.value === undefined) {
        warnings.push('The account sign-in file (.claude.json) could not be read as JSON, so MCP servers were not synced.');
    } else {
        let recorded = {};
        const record = readObject(syncFile).value;
        if (record && record.version === ClaudeConfig.SYNC_VERSION && isPlainObject(record.claude?.mcpServers)) recorded = record.claude.mcpServers;
        const merged = mergeClaudeConfig({ defaultData: defaults.value, profileData: profile.value, synced: recorded, pencil: pencilCommand ? pencilEntry(pencilCommand) : null });
        warnings.push(...merged.warnings);
        if (merged.data) {
            const hasContent = Object.keys(merged.data).length > 0;
            if (profile.value === null && !hasContent) {
                status = 'unchanged';
            } else if (profile.value !== null && sameJson(merged.data, profile.value)) {
                status = 'unchanged';
            } else {
                const indent = /\n( +|\t)"/.exec(profile.text ?? '')?.[1] ?? '  ';
                const endsWithNewline = profile.text === null || profile.text.endsWith('\n');
                const mode = profile.value === null ? 0o600 : fs.statSync(profileFile).mode & 0o777;
                writeAtomically(profileFile, JSON.stringify(merged.data, null, indent) + (endsWithNewline ? '\n' : ''), mode);
                status = profile.value === null ? 'created' : 'updated';
            }
            if (!sameJson(merged.synced, recorded)) {
                writeAtomically(syncFile, `${JSON.stringify({ version: ClaudeConfig.SYNC_VERSION, claude: { mcpServers: merged.synced } }, null, 2)}\n`, 0o600);
            }
        }
    }
    return { status, warnings };
}

ClaudeConfig.PYTHON = String.raw`import json, os, sys, tempfile
default_file, profile_file, sync_file, pencil_command = sys.argv[1:5]
managed = ${JSON.stringify(ClaudeConfig.MANAGED_SERVERS)}
project_keys = ${JSON.stringify(ClaudeConfig.PROJECT_KEYS)}
sync_version = ${ClaudeConfig.SYNC_VERSION}

def read_object(path):
    # Returns (value, text): value is None when the file is absent and False when it exists but is not a JSON object.
    # A refusal is printed as one line without a terminator; the preparation script reports it after its own fixed output.
    try:
        with open(path, encoding='utf-8') as handle:
            text = handle.read()
    except FileNotFoundError:
        return None, None
    try:
        data = json.loads(text)
    except ValueError:
        return False, text
    return (data if isinstance(data, dict) else False), text

def write_atomically(path, text, mode):
    descriptor, temporary = tempfile.mkstemp(dir=os.path.dirname(path) or '.', prefix='.' + os.path.basename(path) + '.', suffix='.tmp')
    try:
        with os.fdopen(descriptor, 'w', encoding='utf-8') as handle:
            handle.write(text)
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

def clone(value):
    return json.loads(json.dumps(value))

if os.path.abspath(default_file) == os.path.abspath(profile_file):
    sys.stdout.write('The default and account sign-in files are the same file, so MCP servers were not synced.')
    sys.exit(0)
default_data, _ = read_object(default_file)
profile_data, profile_text = read_object(profile_file)
if default_data is False:
    sys.stdout.write('The default Claude sign-in file (.claude.json) could not be read as JSON, so MCP servers were not synced.')
    sys.exit(0)
if profile_data is False:
    sys.stdout.write('The account sign-in file (.claude.json) could not be read as JSON, so MCP servers were not synced.')
    sys.exit(0)
recorded = {}
record, _ = read_object(sync_file)
claude_record = record.get('claude') if isinstance(record, dict) else None
if isinstance(record, dict) and record.get('version') == sync_version and isinstance(claude_record, dict) and isinstance(claude_record.get('mcpServers'), dict):
    recorded = claude_record['mcpServers']
data = {} if profile_data is None else clone(profile_data)
if not all(key not in data or isinstance(data[key], dict) for key in ('mcpServers', 'projects')):
    sys.stdout.write('The account sign-in file has an unexpected mcpServers or projects format, so MCP servers were not synced.')
    sys.exit(0)
servers = dict(data.get('mcpServers') or {})
default_servers = default_data.get('mcpServers') if isinstance(default_data, dict) and isinstance(default_data.get('mcpServers'), dict) else {}
next_synced = {}
if default_data is not None:
    for name, value in default_servers.items():
        if name not in managed:
            servers[name] = clone(value)
            next_synced[name] = clone(value)
    for name, value in recorded.items():
        if name not in managed and name not in default_servers and name in servers and servers[name] == value:
            del servers[name]
else:
    next_synced = dict(recorded)
if pencil_command:
    servers[managed[0]] = {'type': 'stdio', 'command': pencil_command, 'args': ['--app', 'paddock', '--agent', 'claudeCodeCLI'], 'env': {}}
if servers or 'mcpServers' in data:
    data['mcpServers'] = servers
if isinstance(default_data, dict) and isinstance(default_data.get('projects'), dict):
    projects = dict(data.get('projects') or {})
    for folder, answers in default_data['projects'].items():
        if folder not in projects and isinstance(answers, dict):
            picked = {key: clone(answers[key]) for key in project_keys if key in answers}
            if picked:
                projects[folder] = picked
    if projects or 'projects' in data:
        data['projects'] = projects
if profile_data is None and not data:
    pass
elif profile_data is None or data != profile_data:
    indent = '  '
    for line in (profile_text or '').splitlines():
        body = line.lstrip(' \t')
        if body.startswith('"') and body != line:
            indent = line[:len(line) - len(body)]
            break
    ends = profile_text is None or profile_text.endswith('\n')
    mode = 0o600 if profile_data is None else os.stat(profile_file).st_mode & 0o777
    write_atomically(profile_file, json.dumps(data, indent=indent, ensure_ascii=False) + ('\n' if ends else ''), mode)
if next_synced != recorded:
    write_atomically(sync_file, json.dumps({'version': sync_version, 'claude': {'mcpServers': next_synced}}, indent=2, ensure_ascii=False) + '\n', 0o600)
`;

module.exports = { ClaudeConfig, mergeClaudeConfig, syncClaudeConfig };
