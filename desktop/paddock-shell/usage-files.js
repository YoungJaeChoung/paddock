/** Reads CLI usage summaries without exposing conversation text or credential files to callers. */
const fs = require('node:fs');
const path = require('node:path');
const usage = require('./usage-model');

class UsageFiles {
    // Codex 세션 기록 끝부분만 읽는다. 마지막 한도 기록은 파일 끝 가까이에 있다.
    static N_TAIL_BYTES = 512 * 1024;
}

function usageError(
    message,
    statusCode = 409,
) {
    const error = new Error(message);
    error.statusCode = statusCode;
    return error;
}

/** Rejects redirected usage paths; missing paths represent records that the CLI has not created yet. */
function assertPlainPath(
    file,
    kind,
) {
    try {
        const entry = fs.lstatSync(file);
        if (entry.isSymbolicLink() || (kind === 'directory' ? !entry.isDirectory() : !entry.isFile())) {
            throw usageError('A usage path is redirected or has the wrong file type. Restore the account folder before retrying.');
        }
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
}

function readJson(
    file,
    fallback,
) {
    let value = fallback;
    try {
        value = JSON.parse(fs.readFileSync(file, 'utf8'));
    } catch (error) {
        if (error.code !== 'ENOENT') throw usageError('A usage record cannot be read. Check its permissions and JSON format.', 503);
    }
    return value;
}

function readTail(
    file,
) {
    const handle = fs.openSync(file, 'r');
    let text = '';
    try {
        const size = fs.fstatSync(handle).size;
        const start = Math.max(0, size - UsageFiles.N_TAIL_BYTES);
        const buffer = Buffer.alloc(size - start);
        fs.readSync(handle, buffer, 0, buffer.length, start);
        text = buffer.toString('utf8');
    } finally {
        fs.closeSync(handle);
    }
    return text;
}

/** Selects the most recently changed session file while keeping directory links outside the scan. */
function latestCodexSession(
    root,
) {
    const newestFirst = (directory, kind) => {
        let entries = [];
        try {
            entries = fs.readdirSync(directory, { withFileTypes: true })
                .filter(entry => kind === 'directory' ? entry.isDirectory() : entry.isFile())
                .map(entry => entry.name).sort().reverse().map(name => path.join(directory, name));
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
        }
        return entries;
    };
    const files = [];
    let n_days = 0;
    scan: for (const year of newestFirst(root, 'directory')) {
        for (const month of newestFirst(year, 'directory')) {
            for (const day of newestFirst(month, 'directory')) {
                files.push(...newestFirst(day, 'file').filter(file => file.endsWith('.jsonl')).slice(0, 50 - files.length));
                n_days += 1;
                if (n_days >= 2 || files.length >= 50) break scan;
            }
        }
    }
    let latest = null;
    for (const file of files) {
        const modified = fs.statSync(file).mtimeMs;
        if (!latest || modified > latest.modified) latest = { file, modified };
    }
    return latest;
}

function codexUsage(
    roots,
) {
    let result = { windows: [], updatedAt: null };
    try {
        const session = roots.map(root => latestCodexSession(root)).filter(Boolean).sort((left, right) => right.modified - left.modified)[0];
        if (session) result = { windows: usage.codexWindows(readTail(session.file)), updatedAt: Math.floor(session.modified / 1000) };
    } catch (error) {
        // 삭제된 기록은 빈 목록이다. 권한 오류는 조회 실패로 알려 오래된 값과 혼동하지 않게 한다.
        if (error.code !== 'ENOENT') throw usageError('Codex usage records cannot be read. Check the account folder permissions.', 503);
    }
    return result;
}

module.exports = { assertPlainPath, usageError, readJson, latestCodexSession, codexUsage };
