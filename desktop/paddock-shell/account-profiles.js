/**
 * Prepares named CLI environments without reading, copying, or checking login credentials.
 * The filesystem engine stays separate from the RPC entry so only account actions are exposed to clients.
 */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { randomUUID } = require('node:crypto');
const { WslScripts, runWsl } = require('./account-profile-wsl');
const { BrowserBridge } = require('./account-browser');
const { flagSettingsPath } = require('./claude-flag-settings');
const { PencilServer, PencilDoctor, toWslPath, repairPencilCommand, findWindowsPencil } = require('./account-pencil');
const { ClaudeConfig, syncClaudeConfig } = require('./account-claude-config');
const { inspectDirectories, repairDirectory, storageAccessResult, openFolderSettings } = require('./account-storage-access');

class AccountProfile {
    static SERVICE_PATH = '/services/paddock-accounts';
    static PROVIDERS = Object.freeze({ claude: 'Claude', codex: 'Codex' });
    static ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    static METADATA_FILE = 'account-profiles.json';
    static N_LABEL_CHARACTERS = 64;
    // Folders the CLI loads by name from its config folder; each one is linked whole so entries added later appear without re-linking.
    static SHARED_FOLDERS = Object.freeze({ claude: ['commands', 'agents', 'hooks', 'rules'], codex: ['prompts', 'rules'] });
    // Instruction and keybinding files the CLI loads by name from its config folder (Claude Code resolves the user CLAUDE.md and keybindings.json against CLAUDE_CONFIG_DIR, so an account without a link loses them).
    // Settings files the CLI itself rewrites. They are linked like SHARED_FILES, but an account's own copy (made by the CLI before it was linked) is moved aside as a backup first instead of being kept.
    static SETTINGS_FILES = Object.freeze({ claude: ['settings.json'], codex: ['config.toml'] });
    static BACKUP_SUFFIX = '.paddock-backup';
    static SHARED_FILES = Object.freeze({ claude: ['CLAUDE.md', 'keybindings.json'], codex: ['AGENTS.md', 'hooks.json'] });
}

function checkedLabel(
    value,
) {
    if (typeof value !== 'string' || !value.trim() || value.trim().length > AccountProfile.N_LABEL_CHARACTERS || /[\u0000-\u001f\u007f]/.test(value)) {
        throw new Error('Enter an account name between 1 and 64 characters without control characters.');
    }
    return value.trim();
}

function checkedId(
    value,
) {
    if (typeof value !== 'string' || !AccountProfile.ID.test(value)) throw new Error('The account identifier is invalid. Select an account from the list.');
    return value;
}

function checkedProfile(
    value,
) {
    if (!value || typeof value !== 'object' || Array.isArray(value) || !Object.hasOwn(AccountProfile.PROVIDERS, value.provider)) {
        throw new Error('Choose Claude or Codex as the account provider.');
    }
    if (value.runtime !== 'native' && value.runtime !== 'wsl') throw new Error('Choose a native or WSL account environment.');
    const profile = { id: checkedId(value.id), provider: value.provider, label: checkedLabel(value.label), runtime: value.runtime };
    if (value.runtime === 'wsl') {
        if (typeof value.wslDistribution !== 'string' || !value.wslDistribution.trim() || /[\u0000-\u001f\u007f]/.test(value.wslDistribution)) {
            throw new Error('The saved WSL distribution is invalid. Add the account again.');
        }
        profile.wslDistribution = value.wslDistribution;
    }
    return profile;
}

/** Managed directories may not redirect through links into another account's configuration. */
function privateDirectory(
    directory,
) {
    let existing = null;
    try {
        existing = fs.lstatSync(directory);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    if (existing && (!existing.isDirectory() || existing.isSymbolicLink())) throw new Error('An account folder is not a regular directory. Restore the folder before trying again.');
    if (existing && process.platform !== 'win32' && (existing.mode & 0o700) !== 0o700) throw new Error('Account folder access is restricted. Allow access before opening this account.');
    if (!existing) fs.mkdirSync(directory, { mode: 0o700 });
    if (process.platform !== 'win32') fs.chmodSync(directory, 0o700);
}

/**
 * Links one host folder (for example commands) or file (for example AGENTS.md) into an account's config folder.
 *
 * The CLI looks these entries up by name inside its config folder, so an account that lacks the link loses them.
 * Nothing is linked when the host has no such entry; an existing account entry that is not this link is kept and reported.
 * `kind` is 'folder' or 'file'. Returns a warning text when an account entry was kept or a file link was refused, otherwise null.
 */
function linkSharedEntry(
    source,
    target,
    name,
    kind,
) {
    let isFolder = false;
    try {
        const info = fs.statSync(source);
        isFolder = kind === 'folder' ? info.isDirectory() : info.isFile();
    } catch (error) {
        if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
    }
    let warning = null;
    if (isFolder) {
        let existing = null;
        try {
            existing = fs.lstatSync(target);
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
        }
        let sameSource = false;
        // A link whose target is gone is one Paddock made before the host entry moved (a dotfiles repository relocated); it is repointed, not kept.
        let isDangling = false;
        if (existing?.isSymbolicLink()) {
            try {
                sameSource = fs.realpathSync(target) === fs.realpathSync(source);
            } catch (error) {
                if (error.code !== 'ENOENT') throw error;
                isDangling = true;
            }
        }
        if (isDangling) fs.unlinkSync(target);
        if (!existing || isDangling) {
            try {
                fs.symlinkSync(source, target, kind === 'file' ? 'file' : process.platform === 'win32' ? 'junction' : 'dir');
            } catch (error) {
                // Windows allows file links only with developer mode or elevation.
                if (kind !== 'file' || error.code !== 'EPERM') throw error;
                warning = `The ${name} file could not be linked. Enable Windows developer mode to share it.`;
            }
        } else if (!sameSource) warning = `An existing ${name} ${kind} was kept.`;
    }
    return warning;
}

/**
 * Returns the path a settings file is moved to before the account follows the host's file:
 * `<file>.paddock-backup`, or `<file>.paddock-backup-<YYYYMMDD-HHMMSS>` when that name is already taken.
 */
function backupName(
    file,
    now = new Date(),
) {
    const taken = path => {
        let isTaken = true;
        try { fs.lstatSync(path); } catch (error) {
            if (error.code !== 'ENOENT') throw error;
            isTaken = false;
        }
        return isTaken;
    };
    const first = `${file}${AccountProfile.BACKUP_SUFFIX}`;
    let name = first;
    if (taken(first)) {
        const pad = value => String(value).padStart(2, '0');
        name = `${first}-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
    }
    return name;
}

/**
 * Makes an account's settings file follow the host CLI's file, so `/model`, `/theme` and permission grants made in any account land in one file.
 *
 * The CLI rewrites settings files itself, so each account would otherwise keep a diverging copy. The account file becomes a link straight to
 * the host file's final location (the host path with every link resolved). One hop matters: the CLI saves through a link by resolving a single
 * level and renaming over that path, so a link that leads through the host's own link (for example a dotfiles-repository link) would replace
 * the host link with a plain file and cut it off from its repository.
 * Nothing happens when the host has no such file. An account's own regular file is moved to `<name>.paddock-backup`
 * (`-<YYYYMMDD-HHMMSS>` appended when that name is taken) before linking, unless its content already equals the host's.
 * A link that already leads to the host file through another path is made direct; a link to some other file, or a folder, is kept and reported.
 * Windows without developer mode refuses file links (EPERM); then the account gets a copy that is refreshed whenever it differs,
 * and the first differing account file is backed up once.
 * Returns a warning text when something was moved, kept, or copied instead of linked; otherwise null.
 *
 * Examples
 * --------
 * | Host file | Account file                      | Result                                                           |
 * | --------- | --------------------------------- | ---------------------------------------------------------------- |
 * | exists    | none                              | direct link, no warning                                          |
 * | exists    | regular, different content        | account file moved to `.paddock-backup`, direct link, warning    |
 * | exists    | regular, same content             | relinked without backup, no warning                              |
 * | exists    | direct link to the host's file    | unchanged                                                        |
 * | exists    | link to it through another path   | made direct, no warning                                          |
 * | exists    | link elsewhere, or a folder       | kept, warning                                                    |
 * | none      | any                               | unchanged                                                        |
 */
function shareSettingsFile(
    source,
    target,
    name,
) {
    let resolved = null;
    try {
        if (fs.statSync(source).isFile()) resolved = fs.realpathSync(source);
    } catch (error) {
        if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
    }
    let warning = null;
    if (resolved) {
        let existing = null;
        try {
            existing = fs.lstatSync(target);
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
        }
        let isDirect = false;
        let isSameFile = false;
        // A link whose target no longer exists is one Paddock made before the host file moved (for example a relocated dotfiles repository), so it is repointed.
        let isDangling = false;
        if (existing?.isSymbolicLink()) {
            isDirect = path.resolve(path.dirname(target), fs.readlinkSync(target)) === resolved;
            try {
                isSameFile = fs.realpathSync(target) === resolved;
            } catch (error) {
                if (error.code !== 'ENOENT') throw error;
                isDangling = true;
            }
        }
        if (existing?.isSymbolicLink() && !isSameFile && !isDangling) {
            warning = `An existing ${name} link was kept.`;
        } else if (existing && !existing.isSymbolicLink() && !existing.isFile()) {
            warning = `An existing ${name} entry was kept.`;
        } else if (!isDirect) {
            const sameContent = isSameFile || isDangling || (Boolean(existing) && fs.readFileSync(resolved).equals(fs.readFileSync(target)));
            // The link is built beside the account file and renamed over it, so a refused link leaves the account file untouched.
            const pendingLink = path.join(path.dirname(target), `.${name}.${randomUUID()}.link`);
            let canLink = true;
            try {
                fs.symlinkSync(resolved, pendingLink, 'file');
            } catch (error) {
                // Windows allows file links only with developer mode or elevation.
                if (error.code !== 'EPERM') throw error;
                canLink = false;
            }
            try {
                if (canLink) {
                    let backup = null;
                    if (existing && !sameContent) {
                        backup = backupName(target);
                        fs.renameSync(target, backup);
                    }
                    fs.renameSync(pendingLink, target);
                    if (backup) warning = `The ${name} of this account was moved to ${path.basename(backup)}; the account now follows the shared ${name}.`;
                } else if (!sameContent) {
                    const backup = existing && !fs.existsSync(`${target}${AccountProfile.BACKUP_SUFFIX}`) ? `${target}${AccountProfile.BACKUP_SUFFIX}` : null;
                    if (backup) fs.copyFileSync(target, backup, fs.constants.COPYFILE_EXCL);
                    const temporaryFile = path.join(path.dirname(target), `.${name}.${randomUUID()}.tmp`);
                    try {
                        fs.copyFileSync(resolved, temporaryFile);
                        fs.renameSync(temporaryFile, target);
                    } finally {
                        fs.rmSync(temporaryFile, { force: true });
                    }
                    warning = `Enable Windows developer mode to share ${name} live; the account copy is refreshed each launch.`;
                }
            } finally {
                fs.rmSync(pendingLink, { force: true });
            }
        }
    }
    return warning;
}

/**
 * Runs one account sharing step and turns its failure into a warning, so a link or file that cannot be shared never keeps the account from opening.
 * `step` returns a warning text, a list of warning texts, or nothing. Returns the warnings as a list (empty when the step had nothing to report).
 *
 * Examples
 * --------
 * | step                          | result                                                   |
 * | ----------------------------- | -------------------------------------------------------- |
 * | returns `null`                | `[]`                                                     |
 * | returns `'kept'`              | `['kept']`                                               |
 * | throws `EACCES: denied`       | `['commands could not be shared: EACCES: denied']`       |
 */
function sharingStep(
    label,
    step,
) {
    let warnings = [];
    try {
        const result = step();
        if (typeof result === 'string') warnings = [result];
        else if (Array.isArray(result)) warnings = result;
    } catch (error) {
        warnings = [`${label} could not be shared: ${error instanceof Error ? error.message : String(error)}`];
    }
    return warnings;
}

/**
 * Keeps the fixed helper commands (browser opener, and for Claude the Pencil diagnostic) beside their account
 * without following a replaced helper into another file.
 */
function installBrowserBridge(
    configDir,
    provider,
) {
    const browserDirectory = path.join(configDir, 'paddock-bin');
    privateDirectory(browserDirectory);
    const helpers = [[BrowserBridge.NAME, BrowserBridge.SCRIPT], ...(provider === 'claude' ? [[PencilDoctor.NAME, PencilDoctor.SCRIPT]] : [])];
    for (const [name, script] of helpers) {
        const commandFile = path.join(browserDirectory, name);
        let existing = null;
        try {
            existing = fs.lstatSync(commandFile);
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
        }
        if (existing && (!existing.isFile() || existing.isSymbolicLink())) throw new Error('The account browser helper is not a regular file. Restore the helper before trying again.');
        const temporaryFile = path.join(browserDirectory, `.${name}.${randomUUID()}.tmp`);
        try {
            fs.writeFileSync(temporaryFile, script, { mode: 0o700, flag: 'wx' });
            fs.renameSync(temporaryFile, commandFile);
        } finally {
            fs.rmSync(temporaryFile, { force: true });
        }
    }
    return browserDirectory;
}

class AccountProfiles {
    /** Config locations come from the host application; frontend requests never choose filesystem roots. */
    constructor(
        { configDirectory, homeDirectory, claudeDirectory, claudeConfigFile, codexDirectory, windowsUsersDirectory } = {},
    ) {
        this.homeDirectory = path.resolve(homeDirectory || os.homedir());
        this.configDirectory = path.resolve(configDirectory || process.env.THEIA_CONFIG_DIR || path.join(this.homeDirectory, '.paddock', 'config'));
        this.claudeDirectory = path.resolve(claudeDirectory || process.env.CLAUDE_CONFIG_DIR || path.join(this.homeDirectory, '.claude'));
        this.codexDirectory = path.resolve(codexDirectory || process.env.CODEX_HOME || path.join(this.homeDirectory, '.codex'));
        // The default environment's sign-in file (MCP servers, folder trust). The CLI keeps it inside CLAUDE_CONFIG_DIR when that is set, otherwise beside ~/.claude.
        // An explicitly given Claude folder (tests, embedding hosts) is not combined with this process's CLAUDE_CONFIG_DIR.
        this.claudeConfigFile = path.resolve(claudeConfigFile || (claudeDirectory ? path.join(this.homeDirectory, '.claude.json')
            : process.env.CLAUDE_CONFIG_DIR ? path.join(process.env.CLAUDE_CONFIG_DIR, '.claude.json') : path.join(this.homeDirectory, '.claude.json')));
        // Where Windows user folders appear inside WSL; used to find the Pencil server installed by the Windows app.
        this.windowsUsersDirectory = windowsUsersDirectory || '/mnt/c/Users';
        this.metadataFile = path.join(this.configDirectory, AccountProfile.METADATA_FILE);
        this.pendingMutation = Promise.resolve();
        this.accessDenials = new Map();
    }

    /** Returns display metadata only. Missing metadata is a first run; corrupt metadata is never silently reset. */
    async list() {
        this.accessDenials.delete(this.accessKey({}));
        let profiles;
        try { profiles = this.readProfiles(); } catch (error) {
            this.rememberAccessDenial(error, {});
            throw error;
        }
        return profiles;
    }

    async create(
        input,
    ) {
        const profile = await this.mutate(async () => {
            if (!input || typeof input !== 'object' || Array.isArray(input) || Object.keys(input).some(key => !['provider', 'label', 'runtime'].includes(key))) {
                throw new Error('Account creation accepts only provider, name, and runtime.');
            }
            const profiles = this.readProfiles();
            const providerName = AccountProfile.PROVIDERS[input.provider];
            if (!Object.hasOwn(AccountProfile.PROVIDERS, input.provider)) throw new Error('Choose Claude or Codex as the account provider.');
            let n_accounts = 1;
            while (profiles.some(item => item.provider === input.provider && item.label.toLowerCase() === `${providerName} ${n_accounts}`.toLowerCase())) n_accounts += 1;
            const candidate = {
                id: randomUUID(), provider: input.provider, label: input.label === undefined ? `${providerName} ${n_accounts}` : input.label,
                runtime: input.runtime || 'native',
            };
            if (candidate.runtime === 'wsl') {
                if (process.platform !== 'win32') throw new Error('WSL accounts can only be added from Windows.');
                const identity = await runWsl(WslScripts.IDENTITY, [], null);
                candidate.wslDistribution = identity[1];
            }
            const next = checkedProfile(candidate);
            this.assertUniqueLabel(profiles, next);
            // The account is visible only after its environment exists. Failed preparation leaves no saved entry.
            await this.prepareProfile(next);
            this.writeProfiles([...profiles, next]);
            return next;
        }, { runtime: input?.runtime || 'native' });
        return profile;
    }

    async rename(
        id,
        label,
    ) {
        const profile = await this.mutate(async () => {
            const profiles = this.readProfiles();
            const current = this.findProfile(profiles, id);
            const next = { ...current, label: checkedLabel(label) };
            this.assertUniqueLabel(profiles, next);
            this.writeProfiles(profiles.map(item => item.id === next.id ? next : item));
            return next;
        }, { accountId: id });
        return profile;
    }

    /** Removes the launcher entry only; the official CLI retains its login, settings, and conversation files. */
    async remove(
        id,
    ) {
        await this.mutate(async () => {
            const profiles = this.readProfiles();
            this.findProfile(profiles, id);
            this.writeProfiles(profiles.filter(item => item.id !== id));
        }, { accountId: id });
    }

    async prepare(
        id,
    ) {
        const prepared = await this.mutate(async () => {
            const profile = this.findProfile(this.readProfiles(), id);
            const result = await this.prepareProfile(profile);
            return result;
        }, { accountId: id });
        return prepared;
    }

    /** Resolves access requests from saved account IDs or a new-account runtime, never a client path. */
    checkedAccessRequest(
        request,
    ) {
        if (!request || typeof request !== 'object' || Array.isArray(request)
            || Object.keys(request).some(key => !['accountId', 'runtime'].includes(key))
            || (request.accountId !== undefined && request.runtime !== undefined)) throw new Error('Choose an account or account environment to check access.');
        if (request.accountId !== undefined) checkedId(request.accountId);
        if (request.runtime !== undefined && !['native', 'wsl'].includes(request.runtime)) throw new Error('Choose a native or WSL account environment.');
        return request;
    }

    metadataDirectories() {
        const base = path.join(this.homeDirectory, '.paddock');
        const directories = [];
        if (this.configDirectory === base || this.configDirectory.startsWith(`${base}${path.sep}`)) directories.push(base);
        directories.push(this.configDirectory);
        return directories;
    }

    accessKey(
        request,
    ) {
        return request.accountId ? `account:${request.accountId}` : request.runtime ? `new:${request.runtime}` : 'list';
    }

    /** Windows access() ignores ACLs, so an actual denied file operation supplies the permission evidence. */
    rememberAccessDenial(
        error,
        request,
    ) {
        const base = path.join(this.homeDirectory, '.paddock');
        if (process.platform === 'win32' && ['EACCES', 'EPERM'].includes(error.code) && typeof error.path === 'string') {
            const failed = path.resolve(error.path);
            if (failed === this.configDirectory || failed.startsWith(`${this.configDirectory}${path.sep}`)
                || (failed === base && this.configDirectory.startsWith(`${base}${path.sep}`))) {
                let directory = failed;
                try { if (!fs.lstatSync(failed).isDirectory()) directory = path.dirname(failed); } catch { /* A denied or missing folder remains an OS settings target. */ }
                this.accessDenials.set(this.accessKey(request), { directory, time: Date.now() });
                while (this.accessDenials.size > 32) this.accessDenials.delete(this.accessDenials.keys().next().value);
            }
        }
    }

    accessDirectories(
        profile,
    ) {
        const directories = this.metadataDirectories();
        if (!profile || profile.runtime === 'native') directories.push(path.join(this.configDirectory, 'agent-profiles'));
        if (profile?.runtime === 'native') {
            const accountDirectory = path.join(this.configDirectory, 'agent-profiles', profile.id);
            const configDir = path.join(accountDirectory, profile.provider);
            directories.push(accountDirectory, configDir, path.join(configDir, 'skills'), path.join(configDir, 'paddock-bin'));
        }
        return directories;
    }

    async storageAccessIssue(
        request,
    ) {
        this.checkedAccessRequest(request);
        let directories = this.metadataDirectories();
        let issue = inspectDirectories(directories);
        // Reading the shared account list can fail before a target-specific preparation begins.
        const denied = this.accessDenials.get(this.accessKey(request)) || this.accessDenials.get('list');
        if (!issue && denied && Date.now() - denied.time < 60000) {
            issue = inspectDirectories([...directories, denied.directory]) || { status: 'settings', directory: denied.directory };
        }
        let runtime = 'native';
        let distribution;
        let profile;
        if (!issue) {
            // Metadata is application-owned, but automatic recovery changes directories only.
            try { fs.accessSync(this.metadataFile, fs.constants.R_OK); } catch (error) {
                if (error.code !== 'ENOENT') issue = { status: 'settings', directory: this.configDirectory };
            }
        }
        if (!issue) {
            profile = request.accountId ? this.findProfile(this.readProfiles(), request.accountId) : null;
            runtime = profile?.runtime || request.runtime || 'native';
            if (runtime === 'wsl') {
                if (process.platform !== 'win32') throw new Error('WSL account access can only be checked from Windows.');
                distribution = profile?.wslDistribution || (await runWsl(WslScripts.IDENTITY, [], null))[1];
                const output = await runWsl(WslScripts.ACCESS, ['inspect', profile?.id || '', profile?.provider || ''], distribution);
                if (output[0] !== 'ready') issue = { status: output[0], directory: output[1], reason: output[2] };
                if (!['ready', 'repair', 'settings', 'blocked'].includes(output[0])) throw new Error('WSL account access could not be checked.');
            } else {
                directories = this.accessDirectories(profile);
                issue = inspectDirectories(directories);
            }
        }
        return { issue, runtime, distribution, profile, directories };
    }

    async inspectStorageAccess(
        request,
    ) {
        const { issue, runtime } = await this.storageAccessIssue(request);
        return storageAccessResult(issue, runtime);
    }

    /** Only the explicit access action may restore owner permissions; no CLI-owned files are modified. */
    async allowStorageAccess(
        request,
    ) {
        const result = await this.mutate(async () => {
            let state = await this.storageAccessIssue(request);
            let n_repairs = 0;
            while (state.issue?.status === 'repair' && n_repairs < 16) {
                if (state.runtime === 'wsl') {
                    await runWsl(WslScripts.ACCESS, ['repair', state.profile?.id || '', state.profile?.provider || ''], state.distribution);
                } else repairDirectory(state.directories, state.issue);
                n_repairs += 1;
                state = await this.storageAccessIssue(request);
            }
            if (state.issue) throw new Error(storageAccessResult(state.issue, state.runtime).message);
            return storageAccessResult(null, state.runtime);
        });
        return result;
    }

    async openStorageSettings(
        request,
    ) {
        const { issue, runtime, distribution } = await this.storageAccessIssue(request);
        if (!issue || issue.status !== 'settings') throw new Error('Restore the account folder permissions in its environment, then choose Retry.');
        await openFolderSettings(issue.directory, runtime, distribution);
    }

    findProfile(
        profiles,
        id,
    ) {
        checkedId(id);
        const profile = profiles.find(item => item.id === id);
        if (!profile) throw new Error('This account is no longer available. Refresh the account list.');
        return profile;
    }

    assertUniqueLabel(
        profiles,
        profile,
    ) {
        if (profiles.some(item => item.id !== profile.id && item.provider === profile.provider && item.label.toLowerCase() === profile.label.toLowerCase())) {
            throw new Error('This provider already has an account with that name. Choose a different name.');
        }
    }

    readProfiles() {
        let profiles = [];
        try {
            const metadata = fs.lstatSync(this.metadataFile);
            if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > 1024 * 1024) throw new Error('Invalid metadata file.');
            const saved = JSON.parse(fs.readFileSync(this.metadataFile, 'utf8'));
            if (!saved || saved.version !== 1 || !Array.isArray(saved.profiles)) throw new Error('Invalid metadata format.');
            profiles = saved.profiles.map(checkedProfile);
            if (new Set(profiles.map(profile => profile.id)).size !== profiles.length) throw new Error('Duplicate account identifiers.');
            for (const profile of profiles) this.assertUniqueLabel(profiles, profile);
        } catch (error) {
            if (error.code !== 'ENOENT') {
                const failure = new Error('The saved account list cannot be read. Restore account-profiles.json before changing accounts.');
                // Preserve only the filesystem failure classification for the server's permission recovery path.
                if (['EACCES', 'EPERM'].includes(error.code)) Object.assign(failure, { code: error.code, path: error.path });
                throw failure;
            }
        }
        return profiles;
    }

    writeProfiles(
        profiles,
    ) {
        fs.mkdirSync(this.configDirectory, { recursive: true, mode: 0o700 });
        const temporaryFile = path.join(this.configDirectory, `.${AccountProfile.METADATA_FILE}.${randomUUID()}.tmp`);
        try {
            fs.writeFileSync(temporaryFile, `${JSON.stringify({ version: 1, profiles }, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
            fs.renameSync(temporaryFile, this.metadataFile);
        } finally {
            fs.rmSync(temporaryFile, { force: true });
        }
    }

    /** Serializes asynchronous WSL preparation with metadata edits so concurrent windows cannot lose entries. */
    async mutate(
        action,
        accessRequest,
    ) {
        const result = this.pendingMutation.then(async () => {
            if (accessRequest) this.accessDenials.delete(this.accessKey(accessRequest));
            let value;
            try { value = await action(); } catch (error) {
                if (accessRequest) this.rememberAccessDenial(error, accessRequest);
                throw error;
            }
            return value;
        });
        this.pendingMutation = result.catch(() => {});
        return result;
    }

    async prepareProfile(
        profile,
    ) {
        const result = profile.runtime === 'wsl' ? await this.prepareWsl(profile) : this.prepareNative(profile);
        const environmentName = profile.provider === 'claude' ? 'CLAUDE_CONFIG_DIR' : 'CODEX_HOME';
        const flagSettingsFile = profile.provider === 'claude' ? this.flagSettingsFile(profile, result.configDir) : null;
        return { profile, configDir: result.configDir, env: { [environmentName]: result.configDir }, warnings: result.warnings, ...(result.browserDirectory ? { browserDirectory: result.browserDirectory } : {}), ...(flagSettingsFile ? { flagSettingsFile } : {}), ...(profile.wslDistribution ? { wslDistribution: profile.wslDistribution } : {}) };
    }

    /**
     * Returns where the account's Claude runtime finds Paddock's own settings file (status line and hooks passed with `--settings`).
     * Native accounts use this Paddock's config folder; WSL accounts use `~/.paddock/config` inside the distribution,
     * which is the folder the Windows app writes for WSL (the account folder sits at `~/.paddock/agent-profiles/<id>/claude`).
     * The launcher adds the flag only when the file exists.
     */
    flagSettingsFile(
        profile,
        configDir,
    ) {
        let file;
        if (profile.runtime === 'wsl') {
            const paddockRoot = path.posix.dirname(path.posix.dirname(path.posix.dirname(configDir)));
            file = flagSettingsPath(path.posix.join(paddockRoot, 'config'), path.posix.join);
        } else file = flagSettingsPath(this.configDirectory);
        return file;
    }

    /**
     * Returns the Pencil server program path in the form this host's own Claude sessions can run, or null when Pencil is not installed.
     * Windows hosts use the Windows path; WSL hosts use the /mnt path of the same Windows program.
     */
    pencilCommand() {
        let command = null;
        if (process.platform === 'win32') {
            const candidate = path.join(this.homeDirectory, ...PencilServer.EXECUTABLE);
            if (fs.existsSync(candidate)) command = candidate;
        } else if (process.platform === 'linux' && process.env.WSL_DISTRO_NAME) command = findWindowsPencil(this.windowsUsersDirectory);
        return command;
    }

    prepareNative(
        profile,
    ) {
        const profilesDirectory = path.join(this.configDirectory, 'agent-profiles');
        const accountDirectory = path.join(profilesDirectory, profile.id);
        const configDir = path.join(accountDirectory, profile.provider);
        fs.mkdirSync(path.dirname(this.configDirectory), { recursive: true, mode: 0o700 });
        for (const directory of [this.configDirectory, profilesDirectory, accountDirectory, configDir]) privateDirectory(directory);
        const warnings = [];
        const sourceDirectory = path.join(profile.provider === 'claude' ? this.claudeDirectory : this.codexDirectory, 'skills');
        const targetDirectory = path.join(configDir, 'skills');
        privateDirectory(targetDirectory);
        let entries = [];
        try {
            entries = fs.readdirSync(sourceDirectory, { withFileTypes: true });
        } catch (error) {
            if (error.code !== 'ENOENT') throw new Error('Shared skills could not be read. Check the permissions of the existing skills folder.');
        }
        for (const entry of entries) {
            if (!entry.name.startsWith('.') && entry.name !== 'synced') {
                const source = path.join(sourceDirectory, entry.name);
                const target = path.join(targetDirectory, entry.name);
                let isSkill = false;
                try {
                    isSkill = fs.statSync(source).isDirectory() && fs.statSync(path.join(source, 'SKILL.md')).isFile();
                } catch (error) {
                    if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
                }
                if (isSkill) {
                    let existing = null;
                    try {
                        existing = fs.lstatSync(target);
                    } catch (error) {
                        if (error.code !== 'ENOENT') throw error;
                    }
                    let sameSource = false;
                    if (existing?.isSymbolicLink()) {
                        try {
                            sameSource = fs.realpathSync(target) === fs.realpathSync(source);
                        } catch (error) {
                            if (error.code !== 'ENOENT') throw error;
                        }
                    }
                    if (!existing) fs.symlinkSync(source, target, process.platform === 'win32' ? 'junction' : 'dir');
                    else if (!sameSource) warnings.push(`An existing skill named ${entry.name} was kept.`);
                }
            }
        }
        const hostDirectory = profile.provider === 'claude' ? this.claudeDirectory : this.codexDirectory;
        // Each sharing step fails on its own: a refused link or an unreadable default file becomes a warning and the account still opens.
        // Structure checks (the account folders above) and the browser helper keep throwing because an account on a redirected folder must not start.
        for (const [names, kind] of [[AccountProfile.SHARED_FOLDERS[profile.provider], 'folder'], [AccountProfile.SHARED_FILES[profile.provider], 'file']]) {
            for (const name of names) {
                warnings.push(...sharingStep(name, () => linkSharedEntry(path.join(hostDirectory, name), path.join(configDir, name), name, kind)));
            }
        }
        for (const name of AccountProfile.SETTINGS_FILES[profile.provider]) {
            warnings.push(...sharingStep(name, () => shareSettingsFile(path.join(hostDirectory, name), path.join(configDir, name), name)));
        }
        const browserDirectory = process.platform === 'linux' && process.env.WSL_DISTRO_NAME ? installBrowserBridge(configDir, profile.provider) : null;
        if (profile.provider === 'claude') {
            const command = this.pencilCommand();
            // Pencil's own registration in the default file points at the extension's versioned folder on Windows; the fixed path survives extension updates.
            if (command && process.platform === 'win32') {
                warnings.push(...sharingStep('Pencil', () => { repairPencilCommand(this.claudeConfigFile, path.join(this.configDirectory, 'deployedPlugins'), command); }));
            }
            warnings.push(...sharingStep('MCP servers', () => syncClaudeConfig({
                defaultFile: this.claudeConfigFile,
                profileFile: path.join(configDir, '.claude.json'),
                syncFile: path.join(configDir, ClaudeConfig.SYNC_FILE),
                pencilCommand: command,
            }).warnings));
        }
        return { configDir, warnings, ...(browserDirectory ? { browserDirectory } : {}) };
    }

    /** Returns the Windows Pencil server as a /mnt path for a WSL profile, or an empty text when it is not installed. */
    pencilCommandForWsl() {
        const command = this.pencilCommand();
        return command ? toWslPath(command) ?? '' : '';
    }

    async prepareWsl(
        profile,
    ) {
        if (process.platform !== 'win32') throw new Error('This account requires Windows and its saved WSL distribution.');
        const output = await runWsl(WslScripts.PREPARE, [profile.id, profile.provider, BrowserBridge.NAME, BrowserBridge.SCRIPT, PencilDoctor.NAME, PencilDoctor.SCRIPT, this.pencilCommandForWsl()], profile.wslDistribution);
        const configDir = output.shift();
        const browserDirectory = output.shift();
        if (!configDir || !configDir.startsWith('/') || !configDir.endsWith(`/agent-profiles/${profile.id}/${profile.provider}`)) {
            throw new Error('WSL did not return a valid account folder. Check the distribution and try again.');
        }
        if (browserDirectory !== `${configDir}/paddock-bin`) throw new Error('WSL did not return a valid browser helper folder. Check the distribution and try again.');
        return { configDir, browserDirectory, warnings: output };
    }
}

module.exports = { AccountProfile, AccountProfiles };
