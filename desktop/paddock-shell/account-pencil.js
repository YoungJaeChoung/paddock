/**
 * Describes the Pencil design tool's MCP server for Claude account profiles and ships a diagnostic for it.
 * The Pencil extension writes its registration only to the host user's ~/.claude.json, so a profile that sets
 * CLAUDE_CONFIG_DIR never sees it. The profile registration itself is part of the `.claude.json` sync (account-claude-config.js);
 * this module holds the server description, the default file's path repair, and the lookup of the installed program.
 */
const fs = require('node:fs');
const path = require('node:path');
const { randomUUID } = require('node:crypto');

class PencilServer {
    static NAME = 'pencil';
    static APP = 'paddock';
    static AGENT = 'claudeCodeCLI';
    // The Windows build is the one that reaches the extension's named pipe, so WSL profiles run it through /mnt/<drive>.
    static EXECUTABLE = ['.pencil', 'mcp', 'paddock', 'out', 'mcp-server-windows-x64.exe'];
}

/** Maps C:\Users\me\... to /mnt/c/Users/me/... for a process that runs inside WSL. */
function toWslPath(
    windowsPath,
) {
    const match = /^([a-z]):[\\/](.*)$/i.exec(windowsPath);
    return match ? `/mnt/${match[1].toLowerCase()}/${match[2].replace(/\\/g, '/')}` : null;
}

function pencilEntry(
    command,
) {
    return { type: 'stdio', command, args: ['--app', PencilServer.APP, '--agent', PencilServer.AGENT], env: {} };
}

/**
 * Points the default `.claude.json`'s Pencil server at the fixed Paddock path when the Pencil extension registered a versioned one.
 *
 * The extension registers the server from its own versioned install folder (`<deployedPlugins>/highagency.pencildev@<version>/...`),
 * which disappears when the extension updates; the fixed path under `~/.pencil/mcp/paddock` survives updates. Only a command that lies
 * under the Paddock-deployed extension folder is replaced, and only when the fixed program exists, so a Pencil registered by another app
 * (for example Cursor) is never touched. The file also holds sign-in state: it is read again right before writing, written through a
 * temporary file and rename, and keeps its indentation and permissions. A missing, unparsable, or differently shaped file is left alone.
 * Returns 'updated' | 'unchanged' | 'skipped'.
 *
 * Examples
 * --------
 * | registered command                                         | stableCommand | result    |
 * | ---------------------------------------------------------- | ------------- | --------- |
 * | `<deployedPlugins>/highagency.pencildev@0.6.75/.../x.exe`  | `~/.pencil/.../x.exe` | updated |
 * | `~/.pencil/.../x.exe`                                      | same          | unchanged |
 * | `C:/Cursor/.../x.exe`                                      | `~/.pencil/.../x.exe` | skipped |
 */
function repairPencilCommand(
    claudeFile,
    deployedPluginsDirectory,
    stableCommand,
) {
    let result = 'skipped';
    const read = () => {
        let text = null;
        let data = null;
        try {
            text = fs.readFileSync(claudeFile, 'utf8');
            data = JSON.parse(text);
        } catch {
            data = null;
        }
        return { text, data: data !== null && typeof data === 'object' && !Array.isArray(data) ? data : null };
    };
    const isVersioned = (command) => {
        const prefix = path.join(deployedPluginsDirectory, 'highagency.pencildev@');
        const normalize = (value) => (process.platform === 'win32' ? path.normalize(value).toLowerCase() : path.normalize(value));
        return typeof command === 'string' && normalize(command).startsWith(normalize(prefix));
    };
    const first = read();
    const command = first.data?.mcpServers?.[PencilServer.NAME]?.command;
    if (first.data && stableCommand && fs.existsSync(stableCommand) && isVersioned(command) && command !== stableCommand) {
        // The running Claude session may have rewritten the file since the first read, so decide again from a fresh read.
        const fresh = read();
        const entry = fresh.data?.mcpServers?.[PencilServer.NAME];
        if (fresh.data && isVersioned(entry?.command)) {
            fresh.data.mcpServers[PencilServer.NAME] = { ...entry, command: stableCommand };
            const indent = /\n( +|\t)"/.exec(fresh.text)?.[1] ?? '  ';
            const output = JSON.stringify(fresh.data, null, indent) + (fresh.text.endsWith('\n') ? '\n' : '');
            const temporaryFile = path.join(path.dirname(claudeFile), `.${path.basename(claudeFile)}.${randomUUID()}.tmp`);
            try {
                fs.writeFileSync(temporaryFile, output, { mode: fs.statSync(claudeFile).mode & 0o777, flag: 'wx' });
                fs.renameSync(temporaryFile, claudeFile);
            } finally {
                fs.rmSync(temporaryFile, { force: true });
            }
            result = 'updated';
        }
    } else if (first.data && typeof command === 'string' && command === stableCommand) {
        result = 'unchanged';
    }
    return result;
}

/** Finds the Windows Pencil server for a process inside WSL by scanning the Windows user folders. */
function findWindowsPencil(
    usersDirectory,
) {
    let found = null;
    let names = [];
    try {
        names = fs.readdirSync(usersDirectory);
    } catch (error) {
        if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
    }
    for (const name of names) {
        const candidate = path.posix.join(usersDirectory, name, ...PencilServer.EXECUTABLE);
        if (found === null && fs.existsSync(candidate)) found = candidate;
    }
    return found;
}

/** Diagnostic installed beside the browser helper; it checks each layer between a Claude session and the Pencil app in order. */
class PencilDoctor {
    static NAME = 'paddock-doctor-pencil';
    // python3 is the only JSON and pipe-capable tool every Paddock WSL setup already relies on; the program is fixed and takes no arguments.
    static SCRIPT = String.raw`#!/bin/sh
set -eu
if ! command -v python3 >/dev/null 2>&1; then
    printf '%s\n' '[FAIL] python3 was not found. Install python3 in this environment, then run paddock-doctor-pencil again.' >&2
    exit 1
fi
exec python3 - "$@" <<'PY'
import json, os, queue, subprocess, sys, threading, time

TIMEOUT = 10
PIPE_READY = '[TransportClient] connected to'
home = os.path.expanduser('~')
config_dir = os.environ.get('CLAUDE_CONFIG_DIR')
config_file = os.path.join(config_dir, '.claude.json') if config_dir else os.path.join(home, '.claude.json')

def report(layer, ok, message):
    print('[%s] %s: %s' % ('OK' if ok else 'FAIL', layer, message), flush=True)
    return ok

def lines_of(stream, sink):
    for line in iter(stream.readline, ''):
        sink.put(line)

def wait_for(sink, accept, seconds, process):
    # Stops early when the server has exited and everything it printed has been read.
    deadline = time.time() + seconds
    seen = []
    while time.time() < deadline:
        try:
            line = sink.get(timeout=0.2)
        except queue.Empty:
            if process.poll() is not None:
                time.sleep(0.2)
                if sink.empty():
                    break
            continue
        seen.append(line.rstrip())
        if accept(line):
            return line, seen
    return None, seen

def send(process, message):
    process.stdin.write(json.dumps(message) + '\n')
    process.stdin.flush()

def run():
    # Layer 1: the profile registers Pencil and the registered program exists.
    try:
        with open(config_file, encoding='utf-8') as handle:
            entry = json.load(handle).get('mcpServers', {}).get('pencil')
    except (OSError, ValueError, AttributeError):
        return report('1/3 registration', False, '%s is missing or unreadable. Start Claude once in this account, then open a new account terminal in Paddock to register Pencil.' % config_file)
    if not isinstance(entry, dict) or not entry.get('command'):
        return report('1/3 registration', False, 'mcpServers.pencil is not in %s. Open a new account terminal in Paddock to register it (the Pencil extension and its Windows server must be installed).' % config_file)
    command = entry['command']
    if not os.access(command, os.X_OK):
        return report('1/3 registration', False, '%s does not exist or is not executable. Reinstall the Pencil extension in Paddock, then open a new account terminal.' % command)
    report('1/3 registration', True, '%s -> %s' % (config_file, command))

    # Layer 2: the server starts and reaches the Pencil app over its pipe.
    env = dict(os.environ)
    env.update(entry.get('env') or {})
    try:
        process = subprocess.Popen([command] + list(entry.get('args') or []), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    except OSError as error:
        return report('2/3 connection', False, 'the server could not start (%s). Check that this environment can run Windows programs.' % error)
    errors, outputs = queue.Queue(), queue.Queue()
    threading.Thread(target=lines_of, args=(process.stderr, errors), daemon=True).start()
    threading.Thread(target=lines_of, args=(process.stdout, outputs), daemon=True).start()
    try:
        send(process, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'paddock-doctor-pencil', 'version': '1'}}})
        connected, seen = wait_for(errors, lambda line: PIPE_READY in line, TIMEOUT, process)
        if connected is None:
            tail = ' | '.join(seen[-3:]) or 'no output'
            return report('2/3 connection', False, 'the server did not connect (%s). Make sure Paddock is running with the Pencil extension active and the registered program is the Windows .exe, not the Linux build.' % tail)
        report('2/3 connection', True, connected.strip())

        # Layer 3: a tool call round trip reaches the open editor and returns.
        send(process, {'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        send(process, {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {'name': 'get_app_state', 'arguments': {}}})
        def is_reply(line):
            try:
                return json.loads(line).get('id') == 2
            except ValueError:
                return False
        reply, _ = wait_for(outputs, is_reply, TIMEOUT, process)
        if reply is None and process.poll() is not None:
            return report('3/3 round trip', False, 'the server exited before answering (code %s). Run it by hand to see its error.' % process.returncode)
        if reply is None:
            return report('3/3 round trip', False, 'get_app_state got no answer within %ds. Keep the .pen editor tab visible (do not hide it behind a terminal tab in the same pane), then run this again.' % TIMEOUT)
        result = json.loads(reply)
        if 'error' in result or result.get('result', {}).get('isError'):
            return report('3/3 round trip', False, 'get_app_state returned an error: %s' % json.dumps(result.get('error') or result.get('result'))[:300])
        return report('3/3 round trip', True, 'get_app_state answered')
    finally:
        process.kill()

sys.exit(0 if run() else 1)
PY
`;
}

module.exports = { PencilServer, PencilDoctor, toWslPath, pencilEntry, repairPencilCommand, findWindowsPencil };
