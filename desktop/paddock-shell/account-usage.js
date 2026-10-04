/** Resolves registered account usage separately from the default CLI environment. */
const fs = require('node:fs');
const path = require('node:path');
const { AccountProfile } = require('./account-profiles');
const usage = require('./usage-model');
const { assertPlainPath, usageError, readJson, codexUsage } = require('./usage-files');
const { applyStatusLine, writeJson } = require('./claude-usage-settings');

/** Converts only Linux absolute paths on the selected distribution, keeping its home mapping intact. */
function wslFileMapping(
    information,
    distribution,
) {
    const linuxHome = information.linuxHome;
    const windowsHome = information.home;
    if (!information.ready || typeof linuxHome !== 'string' || !linuxHome.startsWith('/') || /[\u0000-\u001f]/.test(linuxHome) || typeof windowsHome !== 'string') {
        throw usageError('The account WSL distribution is unavailable. Start it and try again.', 503);
    }
    const suffix = linuxHome.replace(/\//g, '\\');
    const root = windowsHome.slice(0, windowsHome.length - suffix.length);
    const match = /^\\\\(?:wsl\.localhost|wsl\$)\\([^\\]+)$/i.exec(root);
    if (!windowsHome.endsWith(suffix) || !match || match[1].toLowerCase() !== distribution.toLowerCase()) {
        throw usageError('The account WSL folder does not belong to its saved distribution.', 503);
    }
    const toLocal = file => {
        if (typeof file !== 'string' || !file.startsWith('/') || /[\u0000-\u001f]/.test(file)) throw usageError('The WSL usage path is invalid.');
        const normalized = path.posix.normalize(file);
        return `${root}${normalized.replace(/\//g, '\\')}`;
    };
    return { linuxHome, toLocal };
}

class AccountUsage {
    constructor(
        { accounts, readWslInfo, sourceScript, commandForNative },
    ) {
        this.accounts = accounts;
        this.readWslInfo = readWslInfo;
        this.sourceScript = sourceScript;
        this.commandForNative = commandForNative;
        this.pendingWrites = Promise.resolve();
    }

    /** Reads account metadata and paths without creating files or preparing a terminal. */
    async resolve(
        accountId,
    ) {
        if (typeof accountId !== 'string' || !AccountProfile.ID.test(accountId)) throw usageError('accountId must identify a saved account.', 400);
        const profile = (await this.accounts.list()).find(item => item.id === accountId);
        if (!profile) throw usageError('This account is no longer available. Refresh the account list.', 404);
        let configDir;
        let toLocal = file => file;
        let node = null;
        let managedDirectories;
        if (profile.runtime === 'wsl') {
            const information = await this.readWslInfo(profile.wslDistribution);
            const mapping = wslFileMapping(information, profile.wslDistribution);
            toLocal = mapping.toLocal;
            node = information.node || null;
            const profileRoot = path.posix.join(mapping.linuxHome, '.paddock', 'agent-profiles');
            configDir = path.posix.join(profileRoot, profile.id, profile.provider);
            managedDirectories = [path.posix.join(mapping.linuxHome, '.paddock'), profileRoot, path.posix.join(profileRoot, profile.id), configDir];
        } else {
            const profileRoot = path.join(this.accounts.configDirectory, 'agent-profiles');
            configDir = path.join(profileRoot, profile.id, profile.provider);
            managedDirectories = [this.accounts.configDirectory, profileRoot, path.join(profileRoot, profile.id), configDir];
        }
        for (const directory of managedDirectories) assertPlainPath(toLocal(directory), 'directory');
        const join = profile.runtime === 'wsl' ? path.posix.join : path.join;
        const usageDirectory = join(configDir, 'paddock-usage');
        const settingsPath = join(configDir, 'settings.json');
        const sessionsDirectory = join(configDir, 'sessions');
        assertPlainPath(toLocal(usageDirectory), 'directory');
        assertPlainPath(toLocal(sessionsDirectory), 'directory');
        return { profile, configDir, usageDirectory, settingsPath, sessionsDirectory, toLocal, node, join };
    }

    async read(
        accountId,
    ) {
        const scope = await this.resolve(accountId);
        const result = { claude: { state: 'unset', windows: [], updatedAt: null }, codex: { windows: [], updatedAt: null } };
        if (scope.profile.provider === 'claude') {
            const stateFile = scope.toLocal(scope.join(scope.usageDirectory, 'state.json'));
            const usageFile = scope.toLocal(scope.join(scope.usageDirectory, 'claude.json'));
            for (const file of [stateFile, usageFile]) assertPlainPath(file, 'file');
            const record = readJson(usageFile, null);
            result.claude = { state: readJson(stateFile, {}).claude || 'unset', windows: record ? usage.claudeWindows(record) : [], updatedAt: record?.updated_at ?? null };
        } else {
            result.codex = codexUsage([scope.toLocal(scope.sessionsDirectory)]);
        }
        return result;
    }

    /** Serializes setting changes so checkbox updates cannot overwrite another account's saved command. */
    async setClaude(
        accountId,
        enabled,
        isAutomatic = false,
    ) {
        const operation = this.pendingWrites.then(async () => {
            const scope = await this.resolve(accountId);
            if (scope.profile.provider !== 'claude') throw usageError('Claude usage settings require a Claude account.', 400);
            const script = scope.join(scope.usageDirectory, 'claude-statusline.cjs');
            const usageFile = scope.join(scope.usageDirectory, 'claude.json');
            const previousFile = scope.join(scope.usageDirectory, 'previous-statusline.json');
            const stateFile = scope.join(scope.usageDirectory, 'state.json');
            for (const file of [scope.settingsPath, `${scope.settingsPath}.paddock-backup`, script, usageFile, previousFile, stateFile]) assertPlainPath(scope.toLocal(file), 'file');
            let command = scope.profile.runtime === 'wsl'
                ? scope.node && [scope.node, script, usageFile, previousFile].map(file => usage.quoteStatusLineArgument(file)).join(' ')
                : this.commandForNative(script, usageFile, previousFile);
            if (enabled && !command) throw usageError('Node.js is needed to show Claude usage in this account. Install it in the selected environment and try again.', 503);
            if (!enabled) {
                const current = readJson(scope.toLocal(scope.settingsPath), {}).statusLine?.command;
                const files = usage.paddockStatusLineFiles(current);
                if (files?.script === script && files?.previousFile === previousFile && files?.usageFile === usageFile) command = current;
            }
            if (!fs.existsSync(scope.toLocal(scope.usageDirectory))) fs.mkdirSync(scope.toLocal(scope.usageDirectory), { mode: 0o700 });
            if (enabled) fs.copyFileSync(this.sourceScript, scope.toLocal(script));
            const state = applyStatusLine({ settingsPath: scope.toLocal(scope.settingsPath), previousFile: scope.toLocal(previousFile), command, enabled, isAutomatic, toLocal: scope.toLocal, scoped: true });
            writeJson(scope.toLocal(stateFile), { claude: state });
            return { state };
        });
        this.pendingWrites = operation.catch(() => {});
        return operation;
    }
}

module.exports = { AccountUsage, wslFileMapping };
