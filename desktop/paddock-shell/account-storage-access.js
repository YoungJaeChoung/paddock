/** Checks only application-selected folders; callers never supply a filesystem path over RPC. */
const fs = require('node:fs');
const path = require('node:path');
const { execFile } = require('node:child_process');

class StorageAccess {
    static LINUX_O_PATH = 0x200000;
    static MESSAGES = Object.freeze({
        repair: 'Paddock cannot access its account folders. Allow access restores read, write, and open permission for folders owned by your current user. Login and conversation files stay unchanged.',
        settings: 'Your operating system is blocking account storage. Check the folder owner and permissions, then choose Retry. Paddock will not change another user’s permissions or request administrator access.',
        blocked: 'An account folder is a link or is not a regular directory. Restore the original folder, then choose Retry. Paddock will not change the linked location.',
        ready: '',
    });
}

/** Returns the first inaccessible ancestor so a denied parent is not mistaken for a missing child. */
function inspectDirectories(
    directories,
) {
    const managed = new Set(directories);
    const visited = new Set();
    let issue = null;
    for (const directory of directories) {
        const ancestors = [];
        for (let current = directory; current !== path.dirname(current); current = path.dirname(current)) ancestors.unshift(current);
        for (const current of ancestors) {
            if (!issue && !visited.has(current)) {
                visited.add(current);
                try {
                    const metadata = fs.lstatSync(current);
                    if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
                        issue = { status: 'blocked', directory: current };
                    } else if (managed.has(current) && process.platform !== 'win32' && metadata.uid !== process.getuid()) {
                        issue = { status: 'settings', directory: current };
                    } else if (managed.has(current) && process.platform !== 'win32' && (metadata.mode & 0o700) !== 0o700) {
                        // Node exposes no descriptor-relative directory walk on macOS; its folder UI owns recovery.
                        const status = process.platform === 'linux' ? 'repair' : 'settings';
                        issue = { status, directory: current, device: metadata.dev, inode: metadata.ino };
                    } else {
                        fs.accessSync(current, managed.has(current) ? fs.constants.R_OK | fs.constants.W_OK | fs.constants.X_OK : fs.constants.X_OK);
                    }
                } catch (error) {
                    if (error.code !== 'ENOENT') issue = { status: 'settings', directory: current };
                }
            }
        }
    }
    return issue;
}

/** Pins every Linux ancestor so replacing an intermediate folder with a link cannot redirect a permission change. */
function pinLinuxDirectory(
    directory,
) {
    const flags = StorageAccess.LINUX_O_PATH | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW;
    let descriptor = fs.openSync(path.parse(directory).root, flags);
    try {
        for (const segment of directory.split(path.sep).filter(Boolean)) {
            const next = fs.openSync(`/proc/self/fd/${descriptor}/${segment}`, flags);
            fs.closeSync(descriptor);
            descriptor = next;
        }
    } catch (error) {
        fs.closeSync(descriptor);
        throw error;
    }
    return descriptor;
}

/** Rechecks ownership and the directory entry immediately before changing one managed folder. */
function repairDirectory(
    directories,
    issue,
) {
    const current = inspectDirectories(directories);
    if (process.platform !== 'linux' || !current || current.status !== 'repair' || current.directory !== issue.directory
        || current.device !== issue.device || current.inode !== issue.inode || !directories.includes(current.directory)) {
        throw new Error('The account folder changed or requires operating system permission settings. Try again.');
    }
    let descriptor;
    try {
        descriptor = pinLinuxDirectory(current.directory);
        const metadata = fs.fstatSync(descriptor);
        if (!metadata.isDirectory() || metadata.uid !== process.getuid() || metadata.dev !== current.device || metadata.ino !== current.inode) {
            throw new Error('The account folder changed. Try again without changing the linked location.');
        }
        fs.chmodSync(`/proc/self/fd/${descriptor}`, 0o700);
    } finally {
        if (descriptor !== undefined) fs.closeSync(descriptor);
    }
}

function storageAccessResult(
    issue,
    runtime,
) {
    const status = issue?.status || 'ready';
    let message = StorageAccess.MESSAGES[status];
    if (status === 'settings' && runtime === 'wsl') {
        message = 'WSL is blocking account storage. The folder owner or your WSL administrator must restore Linux folder permissions. Windows privacy settings do not change WSL permissions. Choose Retry after access is restored.';
        if (issue.reason === 'python-unavailable') message += ' Automatic recovery is unavailable because this distribution does not have Python 3. You can restore folder permissions yourself without installing it.';
    } else if (status === 'settings' && process.platform === 'win32') {
        message += ' In folder Properties, check Security. If Windows Security blocked Paddock, review Controlled folder access there.';
    } else if (status === 'settings' && process.platform === 'darwin') {
        message += ' Check Sharing & Permissions in Finder and Files and Folders in System Settings > Privacy & Security.';
    }
    return { status, message, canOpenSettings: status === 'settings', settingsLabel: runtime === 'wsl' || process.platform === 'linux' ? 'Open folder' : 'Open folder settings', ...(status === 'settings' ? { folder: issue.directory } : {}) };
}

/** Opens the operating system's folder UI without elevation or a caller-provided command. */
async function openFolderSettings(
    directory,
    runtime = 'native',
    distribution,
) {
    let target = directory;
    if (runtime === 'wsl') {
        if (process.platform !== 'win32' || typeof distribution !== 'string' || /[\\/\u0000-\u001f]/.test(distribution)
            || !directory.startsWith('/')) throw new Error('The WSL account folder is unavailable. Open its distribution in Windows Terminal.');
        target = `\\\\wsl.localhost\\${distribution}${directory.replaceAll('/', '\\')}`;
    }
    while (!fs.existsSync(target) && target !== path.dirname(target)) target = path.dirname(target);
    let command;
    let args;
    if (runtime === 'wsl') {
        command = 'explorer.exe';
        args = [target];
    } else if (process.platform === 'win32') {
        const encodedPath = Buffer.from(target, 'utf8').toString('base64');
        const script = `$ErrorActionPreference='Stop'; $p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${encodedPath}')); $shell=New-Object -ComObject Shell.Application; $folder=$shell.NameSpace($p); if ($null -eq $folder) { throw 'Folder unavailable' }; $folder.Self.InvokeVerb('properties')`;
        command = 'powershell.exe';
        args = ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')];
    } else if (process.platform === 'darwin') {
        command = '/usr/bin/osascript';
        args = ['-e', 'on run argv\ntell application "Finder"\nactivate\nopen information window of (POSIX file (item 1 of argv) as alias)\nend tell\nend run', target];
    } else {
        command = 'xdg-open';
        args = [target];
    }
    await new Promise((resolve, reject) => {
        execFile(command, args, { timeout: 15000, windowsHide: true, maxBuffer: 16384 }, error => {
            if (error) reject(new Error('Folder settings could not be opened. Open the account storage folder in your file manager, check its permissions, then choose Retry.'));
            else resolve();
        });
    });
}

module.exports = { inspectDirectories, repairDirectory, storageAccessResult, openFolderSettings };
