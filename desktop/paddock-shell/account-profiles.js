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
const { inspectDirectories, repairDirectory, storageAccessResult, openFolderSettings } = require('./account-storage-access');

class AccountProfile {
    static SERVICE_PATH = '/services/paddock-accounts';
    static PROVIDERS = Object.freeze({ claude: 'Claude', codex: 'Codex' });
    static ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    static METADATA_FILE = 'account-profiles.json';
    static N_LABEL_CHARACTERS = 64;
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

/** Keeps the fixed browser opener beside its account without following a replaced helper into another file. */
function installBrowserBridge(
    configDir,
) {
    const browserDirectory = path.join(configDir, 'paddock-bin');
    privateDirectory(browserDirectory);
    const commandFile = path.join(browserDirectory, BrowserBridge.NAME);
    let existing = null;
    try {
        existing = fs.lstatSync(commandFile);
    } catch (error) {
        if (error.code !== 'ENOENT') throw error;
    }
    if (existing && (!existing.isFile() || existing.isSymbolicLink())) throw new Error('The account browser helper is not a regular file. Restore the helper before trying again.');
    const temporaryFile = path.join(browserDirectory, `.${BrowserBridge.NAME}.${randomUUID()}.tmp`);
    try {
        fs.writeFileSync(temporaryFile, BrowserBridge.SCRIPT, { mode: 0o700, flag: 'wx' });
        fs.renameSync(temporaryFile, commandFile);
    } finally {
        fs.rmSync(temporaryFile, { force: true });
    }
    return browserDirectory;
}

class AccountProfiles {
    /** Config locations come from the host application; frontend requests never choose filesystem roots. */
    constructor(
        { configDirectory, homeDirectory, claudeDirectory, codexDirectory } = {},
    ) {
        this.homeDirectory = path.resolve(homeDirectory || os.homedir());
        this.configDirectory = path.resolve(configDirectory || process.env.THEIA_CONFIG_DIR || path.join(this.homeDirectory, '.paddock', 'config'));
        this.claudeDirectory = path.resolve(claudeDirectory || process.env.CLAUDE_CONFIG_DIR || path.join(this.homeDirectory, '.claude'));
        this.codexDirectory = path.resolve(codexDirectory || process.env.CODEX_HOME || path.join(this.homeDirectory, '.codex'));
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
        return { profile, configDir: result.configDir, env: { [environmentName]: result.configDir }, warnings: result.warnings, ...(result.browserDirectory ? { browserDirectory: result.browserDirectory } : {}), ...(profile.wslDistribution ? { wslDistribution: profile.wslDistribution } : {}) };
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
        const browserDirectory = process.platform === 'linux' && process.env.WSL_DISTRO_NAME ? installBrowserBridge(configDir) : null;
        return { configDir, warnings, ...(browserDirectory ? { browserDirectory } : {}) };
    }

    async prepareWsl(
        profile,
    ) {
        if (process.platform !== 'win32') throw new Error('This account requires Windows and its saved WSL distribution.');
        const output = await runWsl(WslScripts.PREPARE, [profile.id, profile.provider, BrowserBridge.NAME, BrowserBridge.SCRIPT], profile.wslDistribution);
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
