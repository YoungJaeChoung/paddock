const usage = require('./usage-model');

class UsageAccounts {
    static PROVIDERS = Object.freeze({ claude: 'Claude', codex: 'Codex' });
    static MARKS = Object.freeze({ claude: '✱', codex: '◎' });
    static N_SEARCH_THRESHOLD = 6;
    static POLL_INTERVAL_MS = 120000;
}

function scopeKey(
    profile,
) {
    return `${profile.provider}:${profile.id || 'default'}`;
}

function accountName(
    profile,
) {
    return profile.id ? profile.label : 'Current CLI';
}

/** Keeps each account's response separate, including failures and responses received after a selection changes. */
class UsageData {
    constructor(
        { fetchJson, listProfiles, isEnabled, onChange = () => {}, isClaudeChosen = () => true, onLegacyDisabled = async () => {}, onAutomaticSetup = () => {} },
    ) {
        this.fetchJson = fetchJson;
        this.listProfiles = listProfiles;
        this.isEnabled = isEnabled;
        this.onChange = onChange;
        this.isClaudeChosen = isClaudeChosen;
        this.onLegacyDisabled = onLegacyDisabled;
        this.onAutomaticSetup = onAutomaticSetup;
        this.profiles = [];
        this.profilesKnown = false;
        this.profilesFailed = false;
        this.selected = { claude: { provider: 'claude' }, codex: { provider: 'codex' } };
        this.active = null;
        this.snapshots = new Map();
        this.pending = new Map();
        this.attemptedHooks = new Map();
        this.failedHooks = new Set();
        this.hookQueue = Promise.resolve();
        this.n_revision = 0;
        this.n_profileRevision = 0;
    }

    select(
        profile,
    ) {
        const next = Object.hasOwn(UsageAccounts.PROVIDERS, profile?.provider) ? { ...profile } : null;
        const previousKey = this.active ? scopeKey(this.active) : null;
        const nextKey = next ? scopeKey(next) : null;
        if (next) this.selected[next.provider] = next;
        this.active = next;
        if (previousKey !== nextKey) {
            // A → B → A must not accept the response from the first A request.
            this.n_revision += 1;
            this.onChange();
        }
    }

    current(
        profile,
    ) {
        return profile.id ? this.profiles.find(item => item.id === profile.id) || profile : profile;
    }

    snapshot(
        profile,
    ) {
        const removed = profile.id && this.profilesKnown && !this.profiles.some(item => item.id === profile.id);
        return removed ? { status: 'unavailable', removed: true } : this.snapshots.get(scopeKey(profile)) || { status: 'loading' };
    }

    async updateProfiles() {
        const n_profileRevision = ++this.n_profileRevision;
        try {
            const profiles = await this.listProfiles();
            if (n_profileRevision === this.n_profileRevision) {
                this.profiles = profiles;
                for (const [provider, selected] of Object.entries(this.selected)) {
                    const current = profiles.find(profile => profile.id === selected.id);
                    if (current) this.selected[provider] = { ...current };
                }
                this.profilesKnown = true;
                this.profilesFailed = false;
            }
        } catch {
            if (n_profileRevision === this.n_profileRevision) this.profilesFailed = true;
        }
        this.onChange();
    }

    preferencesChanged() {
        this.n_revision += 1;
        this.attemptedHooks.clear();
        this.failedHooks.clear();
        this.onChange();
    }

    /**
     * Claude 표시 설정과 계정별 상태 줄 등록을 맞추고, 바뀌면 사용량을 다시 읽는다.
     * 켜면 상태 줄을 넣고 끄면 원래 명령을 복원한다. 이전 버전에서 기본 환경만 꺼 둔 설정도 보존한다.
     * 같은 목표는 계정마다 한 번 시도하여 Node.js가 없어 실패해도 주기마다 다시 설정하지 않는다.
     */
    async syncClaude(
        profile,
        data,
    ) {
        if (!profile.id && data.state === 'off' && !this.isClaudeChosen()) await this.onLegacyDisabled();
        const key = scopeKey(profile);
        const wants = this.isEnabled('claude');
        // An earlier enable may still be finishing, so a global disable is always queued once per account.
        const needsChange = wants ? data.state !== 'on' : true;
        let next = data;
        if (needsChange && this.attemptedHooks.get(key) !== wants) {
            this.attemptedHooks.set(key, wants);
            const action = this.hookQueue.catch(() => {}).then(async () => {
                const enabled = this.isEnabled('claude');
                const query = `?enabled=${enabled}${profile.id ? `&accountId=${encodeURIComponent(profile.id)}` : ''}${enabled && data.state === 'unset' ? '&automatic=true' : ''}`;
                const result = await this.fetchJson('/paddock/usage/claude', 'POST', query);
                // Another Paddock may own the default status line; a skipped automatic setup must not claim success.
                if (!profile.id && enabled && data.state === 'unset' && result.state === 'on') this.onAutomaticSetup();
            });
            this.hookQueue = action;
            try {
                await action;
                this.failedHooks.delete(key);
                const response = await this.fetchJson('/paddock/usage', 'GET', profile.id ? `?accountId=${encodeURIComponent(profile.id)}` : '');
                next = response.claude;
            } catch (error) {
                this.failedHooks.add(key);
                throw error;
            }
        } else if (needsChange && this.failedHooks.has(key)) {
            throw new Error('Usage setup is unavailable.');
        }
        return next;
    }

    async read(
        profile,
    ) {
        const key = scopeKey(profile);
        const n_revision = this.n_revision;
        const existing = this.pending.get(key);
        let request;
        if (existing?.n_revision === n_revision) {
            request = existing.promise;
        } else if (this.snapshot(profile).removed) {
            request = Promise.resolve();
        } else {
            const ticket = { n_revision };
            this.pending.set(key, ticket);
            ticket.promise = (async () => {
                let snapshot;
                try {
                    let data;
                    if (profile.provider === 'claude' && !this.isEnabled('claude')) {
                        // Turning the feature off must restore hooks even when a usage record cannot be read.
                        data = await this.syncClaude(profile, { state: 'unset', windows: [], updatedAt: null });
                    } else {
                        const response = await this.fetchJson('/paddock/usage', 'GET', profile.id ? `?accountId=${encodeURIComponent(profile.id)}` : '');
                        data = response[profile.provider];
                        if (profile.provider === 'claude' && this.n_revision === n_revision) data = await this.syncClaude(profile, data);
                    }
                    if (!data || !Array.isArray(data.windows)) throw new Error('Usage data is unavailable.');
                    snapshot = { status: 'ready', data };
                } catch {
                    snapshot = { status: 'unavailable' };
                }
                if (this.n_revision === n_revision && this.pending.get(key) === ticket) {
                    this.snapshots.set(key, snapshot);
                    this.onChange();
                }
                if (this.pending.get(key) === ticket) this.pending.delete(key);
            })();
            request = ticket.promise;
        }
        await request;
    }

    async refresh(
        { all = false, settings = false } = {},
    ) {
        if (all || settings || !this.profilesKnown) await this.updateProfiles();
        const scopes = new Map(Object.values(this.selected).map(profile => [scopeKey(profile), this.current(profile)]));
        if (all) {
            for (const provider of Object.keys(UsageAccounts.PROVIDERS)) scopes.set(`${provider}:default`, { provider });
            for (const profile of this.profiles) scopes.set(scopeKey(profile), profile);
        }
        if (settings || !this.isEnabled('claude')) {
            scopes.set('claude:default', { provider: 'claude' });
            for (const profile of this.profiles.filter(item => item.provider === 'claude')) scopes.set(scopeKey(profile), profile);
        }
        await Promise.allSettled([...scopes.values()].filter(profile => profile.provider === 'claude' || this.isEnabled(profile.provider)).map(profile => this.read(profile)));
    }
}

function node(
    tag,
    className,
    text,
) {
    const result = document.createElement(tag);
    result.className = className;
    if (text !== undefined) result.textContent = text;
    return result;
}

function action(
    label,
    className,
    callback,
) {
    const result = node('button', className, label);
    result.type = 'button';
    result.addEventListener('click', callback);
    return result;
}

/** Shows the active terminal's account and opens the complete list without switching a conversation. */
class UsagePanel {
    constructor(
        { host, fetchJson, listProfiles, isEnabled, meter, onOpen, onManage, isClaudeChosen, onLegacyDisabled, onAutomaticSetup },
    ) {
        this.host = host;
        this.meter = meter;
        this.onOpen = onOpen;
        this.onManage = onManage;
        this.refreshes = new Map();
        this.lastRefreshAt = 0;
        this.data = new UsageData({ fetchJson, listProfiles, isEnabled, isClaudeChosen, onLegacyDisabled, onAutomaticSetup, onChange: () => this.render() });
        this.popup = node('section', 'account-usage-popup');
        this.popup.popover = 'auto';
        this.popup.setAttribute('role', 'dialog');
        this.popup.setAttribute('aria-label', 'Account usage');
        this.popup.addEventListener('toggle', () => {
            for (const button of this.host.querySelectorAll('button')) button.setAttribute('aria-expanded', String(this.popup.matches(':popover-open')));
        });
        this.popup.addEventListener('keydown', event => {
            if (event.key === 'Escape') {
                event.preventDefault();
                this.close();
            }
        });
        const heading = node('div', 'account-usage-heading');
        const headingActions = node('div', 'account-usage-heading-actions');
        this.retryButton = action('Retry', 'account-usage-retry', () => void this.retry());
        this.retryButton.setAttribute('aria-label', 'Retry usage');
        this.retryButton.hidden = true;
        headingActions.append(this.retryButton, action('Close', 'account-usage-close', () => this.close()));
        heading.append(node('strong', '', 'Account usage'), headingActions);
        this.search = node('input', 'account-usage-search');
        this.search.type = 'search';
        this.search.placeholder = 'Search accounts';
        this.search.setAttribute('aria-label', 'Search accounts');
        this.search.addEventListener('input', () => this.renderRows());
        this.rows = node('div', 'account-usage-rows');
        this.notice = node('p', 'account-usage-notice');
        this.notice.setAttribute('role', 'status');
        const footer = node('div', 'account-usage-actions');
        footer.append(action('Manage accounts…', '', () => {
            this.close();
            this.onManage();
        }));
        this.popup.append(heading, node('p', 'account-usage-hint', 'Usage reported by each CLI.'), this.search, this.notice, this.rows, footer);
        host.parentElement.append(this.popup);
        document.addEventListener('visibilitychange', () => {
            if (!document.hidden) void this.refresh();
        });
        this.render();
    }

    observe(
        widgetId,
        profile,
        program,
    ) {
        const key = `${widgetId || ''}:${profile?.id || program || ''}`;
        if (key !== this.observed) {
            this.observed = key;
            if (profile) this.data.select(profile);
            else if (Object.hasOwn(UsageAccounts.PROVIDERS, program)) this.data.select({ provider: program });
            else this.data.select(null);
            void this.refresh();
        }
    }

    async refresh(
        options = {},
    ) {
        let request = Promise.resolve();
        const due = !options.periodic || (!document.hidden && !this.refreshes.size && Date.now() - this.lastRefreshAt >= UsageAccounts.POLL_INTERVAL_MS);
        if (due) {
            const all = Boolean(options.all || this.popup.matches(':popover-open'));
            const settings = Boolean(options.settings);
            const key = `${this.data.n_revision}:${all}:${settings}`;
            request = this.refreshes.get(key);
            if (!request) {
                this.lastRefreshAt = Date.now();
                request = this.data.refresh({ all, settings }).finally(() => this.refreshes.delete(key));
                this.refreshes.set(key, request);
            }
        }
        await request;
    }

    async retry() {
        if (!this.retryButton.disabled) {
            this.retryButton.disabled = true;
            this.retryButton.textContent = 'Retrying…';
            this.data.preferencesChanged();
            try {
                await this.refresh({ all: true });
            } finally {
                this.retryButton.disabled = false;
                this.retryButton.textContent = 'Retry';
            }
        }
    }

    show(
        trigger,
    ) {
        this.trigger = trigger;
        this.triggerProvider = trigger.dataset.usageProvider;
        this.search.value = '';
        this.renderRows();
        this.popup.showPopover();
        (this.search.hidden ? this.popup.querySelector('button') : this.search).focus();
        void this.refresh({ all: true });
    }

    close() {
        this.popup.hidePopover();
        const trigger = [...this.host.querySelectorAll('button')].find(item => item.dataset.usageProvider === this.triggerProvider && item.offsetWidth)
            || [...this.host.querySelectorAll('button')].find(item => item.offsetWidth);
        trigger?.focus();
    }

    appendValues(
        target,
        profile,
    ) {
        const snapshot = this.data.snapshot(profile);
        const now = Math.floor(Date.now() / 1000);
        const windows = snapshot.status === 'ready' && (profile.provider !== 'claude' || snapshot.data.state === 'on')
            ? usage.currentWindows(snapshot.data.windows, now) : [];
        if (windows.length) {
            for (const window of windows) {
                const elapsed = usage.duration(Math.max(0, now - (snapshot.data.updatedAt || now))).replace(/^0h /, '');
                const updated = snapshot.data.updatedAt ? ` · Usage recorded ${elapsed} ago` : '';
                const title = `${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)} · ${usage.windowName(window.label)}: ${usage.describe(window, now)}${updated}`;
                target.append(this.meter(`${scopeKey(profile)}-${window.label}`, window.label, window.used, title));
            }
        } else {
            const text = snapshot.status === 'unavailable' ? 'Unavailable' : snapshot.status === 'loading' ? 'Loading…' : 'No data';
            const state = node('span', 'account-usage-state', text);
            state.title = snapshot.removed ? 'This account was removed from the list. Its existing terminal is kept.'
                : snapshot.status === 'unavailable' ? 'Usage could not be read. Open Account usage and select Retry.'
                    : 'Usage appears after the CLI reports its limits. No value is inferred from another account.';
            target.append(state);
        }
    }

    render() {
        const selected = this.data.active ? [this.data.current(this.data.active)] : [];
        const enabledProviders = Object.keys(UsageAccounts.PROVIDERS).filter(provider => this.data.isEnabled(provider));
        this.retryButton.hidden = !this.data.profilesFailed && ![...this.data.profiles, ...selected].some(profile => {
            const snapshot = this.data.snapshot(profile);
            return snapshot.status === 'unavailable' && !snapshot.removed && this.data.isEnabled(profile.provider);
        });
        const key = JSON.stringify([selected.map(profile => [profile, this.data.snapshot(profile)]), enabledProviders, Math.floor(Date.now() / 60000)]);
        if (key !== this.barKey) {
            this.barKey = key;
            const focused = this.host.contains(document.activeElement) ? document.activeElement.dataset.usageProvider : null;
            this.host.hidden = false;
            this.host.replaceChildren();
            for (const profile of selected.filter(item => this.data.isEnabled(item.provider))) {
                const group = action('', 'usage-group account-usage-selected', event => this.show(event.currentTarget));
                group.dataset.source = profile.provider;
                group.dataset.usageProvider = profile.provider;
                group.setAttribute('aria-haspopup', 'dialog');
                group.setAttribute('aria-expanded', String(this.popup.matches(':popover-open')));
                group.setAttribute('aria-label', `${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)} usage. Show all accounts.`);
                group.title = `${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)}. Show usage for all accounts.`;
                group.append(node('span', 'source-mark', UsageAccounts.MARKS[profile.provider]), node('span', 'source-name', UsageAccounts.PROVIDERS[profile.provider]), node('span', 'account-usage-name', accountName(profile)));
                this.appendValues(group, profile);
                this.host.append(group);
            }
            if (enabledProviders.length) {
                const compact = action('Usage', 'account-usage-compact', event => this.show(event.currentTarget));
                compact.classList.toggle('is-only', !selected.some(profile => this.data.isEnabled(profile.provider)));
                compact.dataset.usageProvider = 'compact';
                compact.setAttribute('aria-haspopup', 'dialog');
                compact.setAttribute('aria-expanded', String(this.popup.matches(':popover-open')));
                const active = selected.find(profile => this.data.isEnabled(profile.provider));
                if (active) {
                    compact.append(node('span', 'account-usage-compact-name', accountName(active)));
                    compact.title = `${UsageAccounts.PROVIDERS[active.provider]} · ${accountName(active)}. Show usage for all accounts.`;
                    compact.setAttribute('aria-label', `${UsageAccounts.PROVIDERS[active.provider]} · ${accountName(active)} usage. Show all accounts.`);
                }
                this.host.append(compact);
            }
            if (focused) this.host.querySelector(`[data-usage-provider="${focused}"]`)?.focus({ preventScroll: true });
        }
        if (this.popup?.matches(':popover-open')) this.renderRows();
    }

    /** Keeps launch buttons attached while only their account values change during periodic refreshes. */
    renderRows() {
        const query = this.search.value.toLocaleLowerCase().trim();
        const profiles = [...this.data.profiles];
        // An unregistered CLI has no verified link to a saved account, even when both use the same login.
        if (this.data.active && !this.data.active.id) profiles.unshift(this.data.active);
        const key = JSON.stringify([query, profiles.map(profile => [profile, this.data.snapshot(profile), this.data.isEnabled(profile.provider)]),
            this.data.active, this.opening, this.openError, this.data.profilesFailed, Math.floor(Date.now() / 60000)]);
        if (key !== this.rowsKey) {
            this.rowsKey = key;
            const scrollTop = this.rows.scrollTop;
            this.rowNodes ||= new Map();
            this.sectionNodes ||= new Map();
            // Keep an existing query editable if accounts are removed while this list is open.
            this.search.hidden = this.data.profiles.length < UsageAccounts.N_SEARCH_THRESHOLD && !this.search.value;
            this.notice.textContent = this.openError || (this.data.profilesFailed ? 'Accounts could not be loaded. Select Retry.' : '');
            const visibleKeys = new Set();
            for (const [provider, label] of Object.entries(UsageAccounts.PROVIDERS)) {
                let section = this.sectionNodes.get(provider);
                if (!section) {
                    section = node('section', 'account-usage-section');
                    section.dataset.provider = provider;
                    section.append(node('h3', '', label));
                    this.sectionNodes.set(provider, section);
                    this.rows.append(section);
                }
                const matches = profiles.filter(profile => profile.provider === provider
                    && `${label} ${accountName(profile)}`.toLocaleLowerCase().includes(query));
                section.hidden = !matches.length;
                let n_position = 1;
                for (const profile of matches) {
                    const accountKey = scopeKey(profile);
                    visibleKeys.add(accountKey);
                    const entry = this.rowNodes.get(accountKey) || this.createUsageRow(profile);
                    this.updateUsageRow(entry, profile);
                    if (section.children[n_position] !== entry.row) section.insertBefore(entry.row, section.children[n_position] || null);
                    n_position += 1;
                }
            }
            for (const [accountKey, entry] of this.rowNodes) {
                if (!visibleKeys.has(accountKey)) entry.row.remove();
                if (!profiles.some(profile => scopeKey(profile) === accountKey)) this.rowNodes.delete(accountKey);
            }
            this.emptyRows ||= node('p', 'account-usage-hint', 'No matching accounts.');
            if (!visibleKeys.size && !this.emptyRows.isConnected) this.rows.append(this.emptyRows);
            else if (visibleKeys.size) this.emptyRows.remove();
            this.rows.scrollTop = scrollTop;
        }
    }

    createUsageRow(
        profile,
    ) {
        const entry = {
            profile,
            row: node('div', 'account-usage-row'),
            name: node('strong', 'account-usage-row-name'),
            badge: node('span', 'account-usage-selected-badge', 'Selected'),
            environment: node('span', 'account-usage-environment'),
            values: node('div', 'account-usage-values'),
        };
        const summary = node('div', 'account-usage-summary');
        const header = node('div', 'account-usage-row-header');
        const title = node('div', 'account-usage-row-title');
        title.append(entry.name, entry.badge);
        summary.append(title, entry.environment);
        entry.open = action('Open in new terminal', 'account-usage-open', () => void this.open(entry.profile));
        entry.open.dataset.usageFocus = scopeKey(profile);
        header.append(summary, entry.open);
        entry.row.append(header, entry.values);
        this.rowNodes.set(scopeKey(profile), entry);
        return entry;
    }

    updateUsageRow(
        entry,
        profile,
    ) {
        const accountKey = scopeKey(profile);
        const snapshot = this.data.snapshot(profile);
        const enabled = this.data.isEnabled(profile.provider);
        const activeKey = this.data.active ? scopeKey(this.data.active) : null;
        const key = JSON.stringify([profile, snapshot, enabled, activeKey, this.opening, Math.floor(Date.now() / 60000)]);
        if (key !== entry.key) {
            entry.key = key;
            entry.profile = profile;
            entry.name.textContent = accountName(profile);
            entry.name.title = accountName(profile);
            entry.badge.hidden = accountKey !== activeKey;
            entry.environment.textContent = profile.id ? (profile.runtime === 'wsl' ? `WSL · ${profile.wslDistribution}` : 'This device') : 'Not linked to a saved account';
            entry.values.replaceChildren();
            if (enabled) this.appendValues(entry.values, profile);
            else entry.values.append(node('span', 'account-usage-state', 'Hidden in Quick settings'));
            entry.open.textContent = this.opening === accountKey ? 'Opening…' : 'Open in new terminal';
            entry.open.disabled = Boolean(this.opening);
            entry.open.setAttribute('aria-label', `Open ${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)} in a new terminal`);
        }
    }

    async open(
        profile,
    ) {
        if (!this.opening) {
            this.opening = scopeKey(profile);
            this.openError = '';
            this.renderRows();
            try {
                await this.onOpen(profile);
                this.data.select(profile);
                // Keep input focus in the terminal activated by the launch action.
                this.popup.hidePopover();
            } catch {
                this.openError = `Could not open ${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)}. Try Open in new terminal again.`;
            } finally {
                this.opening = false;
                this.renderRows();
            }
        }
    }
}

module.exports = { UsagePanel, UsageData, scopeKey, accountName };
