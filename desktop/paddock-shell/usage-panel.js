const usage = require('./usage-model');
const { scopeName } = require('./agent-account');

class UsageAccounts {
    static PROVIDERS = Object.freeze({ claude: 'Claude', codex: 'Codex' });
    static MARKS = Object.freeze({ claude: '✱', codex: '◎' });
    static N_SEARCH_THRESHOLD = 6;
    static POLL_INTERVAL_MS = 120000;
    static CUSTOM_MESSAGE = 'This CLI runs with a configuration folder that is not a saved account. Usage from the default CLI is not shown for it.';
    static CURRENT_NAME = 'Default';
    static CURRENT_HINT = 'The default CLI (plain claude or codex in a terminal) is signed in to this account.';
    /** 이 시간보다 오래된 기록은 지금 값처럼 보이지 않게 경과 시간을 함께 보인다. */
    static STALE_SECONDS = 3600;
    static RECORDED_HINT = 'Live usage could not be read, so this is the last value recorded in a Codex session. Sign in again in this account if it continues.';
}

/** 사용량 범위의 키. 등록 계정은 id, 미등록 폴더로 실행한 CLI는 'custom', 기본 CLI는 'default'다. */
function scopeKey(
    profile,
) {
    return `${profile.provider}:${profile.id || (profile.custom ? 'custom' : 'default')}`;
}

function accountName(
    profile,
) {
    return scopeName(profile);
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
        this.selected = {};
        /** 도구별로 미등록 폴더를 고르기 직전의 선택. 미등록 폴더에서 벗어나면 이 값으로 돌아간다. */
        this.beforeCustom = {};
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
        // 미등록 폴더는 그 터미널에서 잠깐 쓰는 범위다. 다른 범위로 옮기면 그 전에 고른 범위로 되돌려, 하단 바에 남지 않게 한다.
        for (const [provider, selected] of Object.entries(this.selected)) {
            if (selected.custom && !(next?.custom && next.provider === provider)) this.selected[provider] = this.beforeCustom[provider] || { provider };
        }
        if (next) {
            if (next.custom && !this.selected[next.provider]?.custom) this.beforeCustom[next.provider] = this.selected[next.provider];
            this.selected[next.provider] = next;
        }
        // 터미널 판정의 연결(linked)은 그 터미널에서 기본 CLI가 실행 중인 동안만 믿는다. 판정이 다시 오지 않으면 낡기 때문이다.
        // 셸로 돌아오거나 다른 화면·도구로 옮기면 지운다. 그 뒤에는 계속 읽는 기본 CLI 사용량 응답의 account만 연결 근거가 된다 —
        // 그래야 에이전트를 끄고 다른 계정으로 다시 로그인했을 때 연결이 풀린다.
        let unlinked = false;
        for (const [provider, selected] of Object.entries(this.selected)) {
            if (selected?.linked && next?.provider !== provider) {
                const { linked, ...rest } = selected;
                this.selected[provider] = rest;
                unlinked = true;
            }
        }
        this.active = next;
        if (unlinked && previousKey === nextKey) this.onChange();
        if (previousKey !== nextKey) {
            // A → B → A must not accept the response from the first A request.
            this.n_revision += 1;
            this.onChange();
        }
    }

    /**
     * 범위가 지금 가리키는 계정. 등록 계정은 목록의 최신 항목이고, 기본 CLI는 로그인이 등록 계정 하나와 같다고 확인되면 그 계정이다.
     * 상태 바·목록이 같은 계정을 두 번(기본 CLI와 등록 계정) 보이지 않게 하려고 존재한다. 미등록 폴더는 그대로다.
     */
    current(
        profile,
    ) {
        let result = profile;
        if (profile.id) result = this.profiles.find(item => item.id === profile.id) || profile;
        else if (!profile.custom) result = this.linkedProfile(profile.provider, profile) || profile;
        return result;
    }

    /**
     * 기본 CLI가 그 계정으로 로그인했다고 확인된 등록 계정. 없거나 목록에 없는 계정이면 null이다.
     * 근거는 터미널 판정(`linked`, 같은 실행 환경에서 확인)이 먼저이고, 없으면 기본 CLI 사용량 응답의 `account`다.
     * 터미널 판정은 그 터미널에서 기본 CLI가 실행 중인 동안만 남는다(select가 지운다).
     * 두 근거 모두 백엔드가 같은 로그인의 등록 계정이 정확히 하나일 때만 보낸다.
     */
    linkedProfile(
        provider,
        scope = this.selected[provider],
    ) {
        const fromTerminal = scope && !scope.id && !scope.custom ? scope.linked?.id : null;
        const shared = this.snapshots.get(`${provider}:default`);
        const id = fromTerminal || (shared?.status === 'ready' ? shared.data.account?.id : null);
        return id ? this.profiles.find(item => item.id === id && item.provider === provider) || null : null;
    }

    /**
     * 범위의 사용량 상태. 등록 계정은 기본 CLI 사용량이 같은 계정으로 확인되면 두 값 중 하나만 쓴다 —
     * 자기 값이 아직 없거나, 기본 CLI 쪽이 한도를 가진 더 최근 값이면 기본 CLI 쪽이다. 같은 계정에 두 숫자를 보이지 않기 위해서다.
     * 같은 계정인지는 기본 CLI 응답의 account만으로 정한다 — 이름을 바꾸는 근거(linkedProfile)의 터미널 판정은 쓰지 않는다.
     * 기본 CLI 응답의 값은 이 컴퓨터와 WSL 가운데 가장 최근 기록이라, 백엔드가 account를 빼면(두 환경의 로그인이 다름)
     * 터미널이 그 계정으로 판정돼도 값은 다른 로그인의 것일 수 있기 때문이다.
     * 그때는 이름은 그 계정, 값은 그 계정 폴더의 자체 기록이다. 오래된 기록이면 표시 쪽이 경과 시간을 함께 보인다.
     */
    snapshot(
        profile,
    ) {
        const removed = profile.id && this.profilesKnown && !this.profiles.some(item => item.id === profile.id);
        let result = removed ? { status: 'unavailable', removed: true } : this.snapshots.get(scopeKey(profile)) || { status: 'loading' };
        const shared = profile.id && !removed ? this.snapshots.get(`${profile.provider}:default`) : null;
        if (shared?.status === 'ready' && shared.data.account?.id === profile.id) {
            const newer = shared.data.windows.length && (shared.data.updatedAt ?? 0) > (result.data?.updatedAt ?? 0);
            if (result.status !== 'ready' || newer) result = shared;
        }
        return result;
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
     * 설정이 실패하면 받은 `data`에 한도 값이 있을 때만 그대로 돌려주고, 없으면 오류를 던진다(행은 Unavailable).
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
                const response = await this.fetchJson('/paddock/usage', 'GET', profile.id ? `?accountId=${encodeURIComponent(profile.id)}` : '?provider=claude');
                next = response.claude;
            } catch (error) {
                this.failedHooks.add(key);
                // 설정 쓰기가 실패해도 이미 읽은 기록이 있으면 그 값은 보인다. 다른 Paddock의 상태 줄이 같은 기록을 쓰고 있을 수 있다.
                if (!data.windows?.length) throw error;
            }
        } else if (needsChange && this.failedHooks.has(key) && !data.windows?.length) {
            throw new Error('Usage setup is unavailable.');
        }
        return next;
    }

    /**
     * 등록 Claude 계정의 터미널을 띄우기 전에 Paddock 상태 줄을 그 계정 설정에 넣어 둔다.
     * Claude Code는 시작할 때 읽은 상태 줄 설정만 쓰므로, 주기 갱신이 뒤늦게 넣으면 그 터미널의 대화는 사용량을 남기지 않는다.
     * 방금 만든 계정은 아직 목록에 없어 지운 계정으로 보이므로 목록을 먼저 다시 읽는다.
     * Claude 표시가 꺼져 있거나 다른 도구의 계정이면 아무것도 하지 않는다. 설정에 실패해도 터미널은 그대로 띄우도록 오류를 던지지 않는다.
     */
    async prepareLaunch(
        profile,
    ) {
        if (profile.provider === 'claude' && profile.id && this.isEnabled('claude')) {
            if (!this.profiles.some(item => item.id === profile.id)) await this.updateProfiles();
            await this.read(profile);
        }
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
        } else if (profile.custom) {
            // 미등록 폴더의 사용량은 읽을 곳이 없다. 기본 CLI의 값이나 상태 줄 설정을 대신 쓰지 않는다.
            if (this.snapshots.get(key)?.status !== 'unavailable') {
                this.snapshots.set(key, { status: 'unavailable', message: UsageAccounts.CUSTOM_MESSAGE });
                this.onChange();
            }
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
                        const response = await this.fetchJson('/paddock/usage', 'GET', profile.id ? `?accountId=${encodeURIComponent(profile.id)}` : `?provider=${profile.provider}`);
                        data = response[profile.provider];
                        if (profile.provider === 'claude' && this.n_revision === n_revision) data = await this.syncClaude(profile, data);
                    }
                    if (!data || !Array.isArray(data.windows)) throw new Error('Usage data is unavailable.');
                    snapshot = data.error ? { status: 'unavailable', message: data.error } : { status: 'ready', data };
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
        const scopes = new Map(Object.keys(UsageAccounts.PROVIDERS).map(provider => {
            const profile = this.current(this.selected[provider] || { provider });
            return [scopeKey(profile), profile];
        }));
        // 기본 CLI가 등록 계정으로 보이는 동안에도 기본 CLI 응답을 계속 읽는다. 로그인이 바뀌면 그 응답의 account로 연결이 풀린다.
        for (const provider of Object.keys(UsageAccounts.PROVIDERS)) {
            const chosen = this.selected[provider] || { provider };
            if (!chosen.id && !chosen.custom) scopes.set(scopeKey(chosen), chosen);
        }
        if (all) {
            for (const provider of Object.keys(UsageAccounts.PROVIDERS)) scopes.set(`${provider}:default`, { provider });
            for (const profile of this.profiles) scopes.set(scopeKey(profile), profile);
        }
        if (settings || !this.isEnabled('claude')) {
            scopes.set('claude:default', { provider: 'claude' });
            for (const profile of this.profiles.filter(item => item.provider === 'claude')) scopes.set(scopeKey(profile), profile);
        }
        // 기본 Codex 응답이 이미 같은 계정으로 확인됐고 이번에도 그 응답을 읽으면, 그 계정 폴더는 따로 읽지 않는다.
        // 한도는 계정마다 하나라 같은 값을 codex app-server 두 번으로 읽게 되고, 두 읽기가 조금 달라 주기마다 숫자가 오갈 수 있기 때문이다.
        // 계정 행·상태 바는 snapshot이 기본 CLI 쪽 값을 쓴다. Claude는 계정마다 상태 줄 설정을 맞춰야 해서 그대로 읽는다.
        // 이번 기본 응답에서 연결이 풀렸으면(로그인이 바뀜) 건너뛴 계정을 같은 주기에 이어서 읽는다. 다음 주기까지 'Loading…'으로 남지 않게 한다.
        const sharedCodex = this.snapshots.get('codex:default');
        const linkedCodexId = scopes.has('codex:default') && sharedCodex?.status === 'ready' ? sharedCodex.data.account?.id : null;
        const skippedKey = linkedCodexId ? scopeKey({ provider: 'codex', id: linkedCodexId }) : null;
        const skipped = skippedKey ? scopes.get(skippedKey) : null;
        if (skippedKey) scopes.delete(skippedKey);
        await Promise.allSettled([...scopes.values()].filter(profile => profile.provider === 'claude' || this.isEnabled(profile.provider)).map(profile => this.read(profile)));
        const sharedAfter = this.snapshots.get('codex:default');
        if (skipped && (sharedAfter?.status !== 'ready' || sharedAfter.data.account?.id !== linkedCodexId)) await this.read(skipped).catch(() => {});
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

/** Keeps the last selected account for each CLI visible and opens the complete list without switching a conversation. */
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
        this.popup.append(heading, node('p', 'account-usage-hint', 'Percent of each limit used.'), this.search, this.notice, this.rows, footer);
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
        // 실행 중인 도구와 그 범위가 함께 키에 들어간다. 같은 터미널에서 도구나 계정이 바뀌면 다시 고른다.
        // 기본 CLI의 로그인 연결(linked)이 생기거나 풀려도 다시 고른다.
        const key = `${widgetId || ''}:${program || ''}:${profile ? scopeKey(profile) : ''}:${profile?.linked?.id || ''}`;
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
        this.sectionNodes?.get(this.triggerProvider)?.scrollIntoView({ block: 'start' });
        void this.refresh({ all: true });
    }

    close() {
        this.popup.hidePopover();
        const trigger = [...this.host.querySelectorAll('button')].find(item => item.dataset.usageProvider === this.triggerProvider && item.offsetWidth)
            || [...this.host.querySelectorAll('button')].find(item => item.offsetWidth);
        trigger?.focus();
    }

    /** Names an unregistered CLI after the saved account whose sign-in it verifiably shares; otherwise keeps Default. */
    displayName(
        profile,
    ) {
        const snapshot = this.data.snapshot(profile);
        const matched = !profile.id && snapshot.status === 'ready' ? snapshot.data.account?.label : null;
        return matched || accountName(profile);
    }

    appendValues(
        target,
        profile,
        details = false,
    ) {
        const snapshot = this.data.snapshot(profile);
        const now = Math.floor(Date.now() / 1000);
        // Claude는 사용자가 끈('off') 기록만 숨긴다. 'unset'이어도 값이 있으면 보인다 — 다른 Paddock 설정 폴더의 상태 줄이
        // 같은 기록을 쓰고 있어 이 설정 폴더의 자동 켜기가 물러선 경우다. 오래된 기록이면 아래에서 경과 시간을 함께 보인다.
        const windows = snapshot.status === 'ready' && (profile.provider !== 'claude' || snapshot.data.state !== 'off')
            ? usage.currentWindows(snapshot.data.windows, now) : [];
        // 실시간 값이 아닌 세션 기록이거나 오래된 기록이면 경과 시간을 값 옆에 보이고 흐리게 한다. 말풍선에만 두면 지금 값으로 오해한다.
        const age = windows.length && snapshot.data.updatedAt ? Math.max(0, now - snapshot.data.updatedAt) : 0;
        const stale = Boolean(windows.length) && (Boolean(snapshot.data.recorded) || age >= UsageAccounts.STALE_SECONDS);
        target.classList.toggle('is-stale', stale);
        if (windows.length) {
            const elapsed = usage.duration(age).replace(/^0h /, '');
            const updated = snapshot.data.updatedAt ? ` · Usage recorded ${elapsed} ago` : '';
            const reason = snapshot.data.recorded ? ` · ${UsageAccounts.RECORDED_HINT}` : '';
            for (const window of windows) {
                const title = `${UsageAccounts.PROVIDERS[profile.provider]} · ${this.displayName(profile)} · ${usage.windowName(window.label)}: ${usage.describe(window, now)}${updated}${reason}`;
                target.append(this.meter(`${scopeKey(profile)}-${window.label}`, window.label, window.used, title));
            }
            if (stale) {
                // 상태 바는 짧은 경과 시간만, 목록 행은 같은 내용을 문장으로 보인다.
                const explanation = `Usage recorded ${snapshot.data.updatedAt ? `${elapsed} ago` : 'at an unknown time'}${reason || '. It may have changed since then.'}`;
                const label = details ? node('span', 'account-usage-hint account-usage-age-hint', explanation)
                    : node('span', 'account-usage-age', snapshot.data.updatedAt ? `${usage.shortAge(age)} ago` : 'recorded');
                label.title = explanation;
                target.append(label);
            }
        } else {
            // 사용량 기록이 아직 없다는 뜻이다. 에이전트가 응답을 기다린다는 진행 상태로 읽히지 않게 사용량을 주어로 쓴다.
            const waitingForClaude = profile.provider === 'claude' && snapshot.status === 'ready' && snapshot.data.state === 'on';
            const text = snapshot.status === 'unavailable' ? 'Unavailable' : snapshot.status === 'loading' ? 'Loading…'
                : waitingForClaude ? 'No usage yet' : 'Not reported';
            const state = node('span', 'account-usage-state', text);
            state.title = snapshot.removed ? 'This account was removed from the list. Its existing terminal is kept.'
                : snapshot.status === 'unavailable' ? snapshot.message || 'Usage could not be read. Open Account usage and select Retry.'
                    : waitingForClaude ? 'Send a message in this account’s Claude terminal. Usage appears after Claude reports its limits.'
                        : 'This CLI has not reported usage limits. No value is inferred from another account.';
            target.append(state);
            if (details && snapshot.status !== 'loading') target.append(node('span', 'account-usage-hint', state.title));
        }
    }

    render() {
        const selected = Object.keys(UsageAccounts.PROVIDERS).filter(provider => this.data.selected[provider])
            .map(provider => this.data.current(this.data.selected[provider]));
        const enabledProviders = Object.keys(UsageAccounts.PROVIDERS).filter(provider => this.data.isEnabled(provider));
        this.retryButton.hidden = !this.data.profilesFailed && ![...this.data.profiles, ...selected].some(profile => {
            const snapshot = this.data.snapshot(profile);
            // 미등록 폴더는 다시 읽어도 값이 생기지 않으므로 다시 시도 대상이 아니다.
            return snapshot.status === 'unavailable' && !snapshot.removed && !profile.custom && this.data.isEnabled(profile.provider);
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
                const name = this.displayName(profile);
                group.setAttribute('aria-label', `${UsageAccounts.PROVIDERS[profile.provider]} · ${name} usage. Show all accounts.`);
                group.title = `${UsageAccounts.PROVIDERS[profile.provider]} · ${name}. Show usage for all accounts.`;
                group.append(node('span', 'source-mark', UsageAccounts.MARKS[profile.provider]), node('span', 'source-name', UsageAccounts.PROVIDERS[profile.provider]), node('span', 'account-usage-name', name));
                this.appendValues(group, profile);
                this.host.append(group);
            }
            if (enabledProviders.length && !selected.some(profile => this.data.isEnabled(profile.provider))) {
                const compact = action('Usage', 'account-usage-compact', event => this.show(event.currentTarget));
                compact.classList.toggle('is-only', !selected.some(profile => this.data.isEnabled(profile.provider)));
                compact.dataset.usageProvider = 'compact';
                compact.setAttribute('aria-haspopup', 'dialog');
                compact.setAttribute('aria-expanded', String(this.popup.matches(':popover-open')));
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
        // An unregistered CLI has no verified link to a saved account unless its sign-in matches exactly one saved account
        // (same login identifier, same runtime and WSL distribution). Only that verified link merges it into the account's row,
        // which then carries a Default badge; an unmatched or ambiguous sign-in keeps its own Default row.
        profiles.unshift(...Object.values(this.data.selected).filter(profile => !profile.id && !this.data.current(profile).id));
        const key = JSON.stringify([query, profiles.map(profile => [profile, this.data.snapshot(profile), this.data.isEnabled(profile.provider)]),
            this.data.selected, Object.keys(UsageAccounts.PROVIDERS).map(provider => this.data.linkedProfile(provider)?.id),
            this.opening, this.openError, this.data.profilesFailed, Math.floor(Date.now() / 60000)]);
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
            badge: node('span', 'account-usage-selected-badge', 'In status bar'),
            current: node('span', 'account-usage-selected-badge account-usage-current-badge', UsageAccounts.CURRENT_NAME),
            environment: node('span', 'account-usage-environment'),
            values: node('div', 'account-usage-values'),
        };
        const summary = node('div', 'account-usage-summary');
        const header = node('div', 'account-usage-row-header');
        const title = node('div', 'account-usage-row-title');
        entry.current.title = UsageAccounts.CURRENT_HINT;
        title.append(entry.name, entry.current, entry.badge);
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
        const selected = this.data.selected[profile.provider];
        // 기본 CLI가 이 계정으로 확인되면 상태 바의 기본 CLI는 이 계정 행이다.
        const activeKey = selected ? scopeKey(this.data.current(selected)) : null;
        const isCurrent = Boolean(profile.id) && this.data.linkedProfile(profile.provider)?.id === profile.id;
        const key = JSON.stringify([profile, snapshot, enabled, activeKey, isCurrent, this.opening, Math.floor(Date.now() / 60000)]);
        if (key !== entry.key) {
            entry.key = key;
            entry.profile = profile;
            entry.name.textContent = accountName(profile);
            entry.name.title = accountName(profile);
            entry.badge.hidden = accountKey !== activeKey;
            entry.current.hidden = !isCurrent;
            const signedIn = profile.id ? null : snapshot.data?.account?.label;
            entry.environment.textContent = profile.id ? (profile.runtime === 'wsl' ? `WSL · ${profile.wslDistribution}` : 'This device')
                : profile.custom ? 'Folder set in the terminal' : signedIn ? `Signed in as ${signedIn}` : 'Not linked to a saved account';
            entry.values.replaceChildren();
            if (enabled) this.appendValues(entry.values, profile, true);
            else entry.values.append(node('span', 'account-usage-state', 'Hidden in Quick settings'));
            entry.open.textContent = this.opening === accountKey ? 'Opening…' : 'Open in new terminal';
            entry.open.disabled = Boolean(this.opening);
            // 미등록 폴더는 Paddock이 다시 열 수 있는 환경이 아니다. 열면 기본 CLI가 열리므로 버튼을 숨긴다.
            entry.open.hidden = Boolean(profile.custom);
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
            // A permission dialog must remain fully visible if opening the account needs access recovery.
            this.popup.hidePopover();
            try {
                await this.onOpen(profile);
                this.data.select(profile);
                // Keep input focus in the terminal activated by the launch action.
                this.popup.hidePopover();
            } catch {
                this.openError = `Could not open ${UsageAccounts.PROVIDERS[profile.provider]} · ${accountName(profile)}. Try Open in new terminal again.`;
                this.popup.showPopover();
            } finally {
                this.opening = false;
                this.renderRows();
                if (this.openError) this.rowNodes.get(scopeKey(profile))?.open.focus();
            }
        }
    }
}

module.exports = { UsagePanel, UsageData, scopeKey, accountName };
