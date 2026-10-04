const { AbstractDialog } = require('@theia/core/lib/browser/dialogs');
const { retryAccountStorage } = require('./account-access-dialog');
require('./account-dialog.css');

class AccountLabels {
    static PROVIDERS = { claude: 'Claude', codex: 'Codex' };
    static RUNTIMES = { native: 'This device', wsl: 'WSL' };
    static N_NAME_CHARACTERS = 64;
}

/** Manage account launch entries without reading credentials or claiming that login succeeded. */
class AccountDialog extends AbstractDialog {
    constructor(
        options,
    ) {
        super({ title: 'Accounts', maxWidth: 640 });
        this.options = options;
        this.profiles = [];
        this.loading = true;
        this.busy = false;
        this.error = '';
        this.formVisible = false;
        this.editing = undefined;
        this.removing = undefined;
        this.draft = { provider: 'claude', label: 'Main', runtime: options.defaultRuntime || 'native' };
        this.addClass('paddock-account-dialog');
        this.contentNode.id = 'paddock-account-content';
        this.titleNode.id = 'paddock-account-title';
        const block = this.node.querySelector('.dialogBlock');
        block.setAttribute('role', 'dialog');
        block.setAttribute('aria-modal', 'true');
        block.setAttribute('aria-labelledby', this.titleNode.id);
        this.closeCrossNode.setAttribute('role', 'button');
        this.closeCrossNode.setAttribute('aria-label', 'Close accounts');
        this.closeCrossNode.tabIndex = 0;
        this.closeCrossNode.addEventListener('keydown', event => {
            if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                this.close();
            }
        });
        this.appendCloseButton('Close');
        this.render();
    }

    get value() {
        return undefined;
    }

    onAfterAttach(
        message,
    ) {
        super.onAfterAttach(message);
        void this.load();
    }

    onActivateRequest(
        message,
    ) {
        super.onActivateRequest(message);
        this.closeButton.focus();
    }

    handleEnter() {
        // Native buttons and forms own Enter; the dialog must never submit a different row.
        return false;
    }

    close() {
        if (!this.busy) {
            super.close();
        }
    }

    element(
        tag,
        className,
        text,
    ) {
        const node = this.node.ownerDocument.createElement(tag);
        node.className = className;
        if (text !== undefined) {
            node.textContent = text;
        }
        return node;
    }

    button(
        text,
        key,
        action,
        primary = false,
    ) {
        const button = this.element('button', `theia-button ${primary ? 'main' : 'secondary'}`, text);
        button.type = 'button';
        button.dataset.focus = key;
        button.disabled = this.busy || this.loading;
        button.addEventListener('click', action);
        return button;
    }

    input(
        value,
        key,
        onChange,
    ) {
        const input = this.element('input', 'theia-input paddock-account-input');
        input.type = 'text';
        input.value = value;
        input.maxLength = AccountLabels.N_NAME_CHARACTERS;
        input.required = true;
        input.autocomplete = 'off';
        input.dataset.focus = key;
        input.disabled = this.busy;
        input.addEventListener('input', () => onChange(input.value));
        return input;
    }

    field(
        labelText,
        input,
    ) {
        input.setAttribute('aria-label', labelText);
        const label = this.element('label', 'paddock-account-field');
        label.append(this.element('span', 'paddock-account-field-label', labelText), input);
        return label;
    }

    select(
        values,
        value,
        key,
        onChange,
    ) {
        const select = this.element('select', 'theia-select paddock-account-select');
        select.dataset.focus = key;
        select.disabled = this.busy;
        for (const option of values) {
            const node = this.element('option', '', option.label);
            node.value = option.value;
            select.appendChild(node);
        }
        select.value = value;
        select.addEventListener('change', () => onChange(select.value));
        return select;
    }

    suggestLabel() {
        const labels = new Set(this.profiles.filter(profile => profile.provider === this.draft.provider).map(profile => profile.label.toLowerCase()));
        let label = 'Main';
        let n_accounts = 2;
        while (labels.has(label.toLowerCase())) {
            label = n_accounts === 2 ? 'Extra' : `Account ${n_accounts}`;
            n_accounts += 1;
        }
        return label;
    }

    labelError(
        label,
        provider,
        id,
    ) {
        let error = '';
        if (!label || label.length > AccountLabels.N_NAME_CHARACTERS || /[\u0000-\u001f\u007f]/.test(label)) {
            error = 'Enter an account name between 1 and 64 characters.';
        } else if (this.profiles.some(profile => profile.id !== id && profile.provider === provider && profile.label.toLowerCase() === label.toLowerCase())) {
            error = 'This tool already has an account with that name. Choose another name.';
        }
        return error;
    }

    render() {
        const active = this.node.ownerDocument.activeElement;
        const focusKey = this.node.contains(active) ? active?.dataset.focus : undefined;
        const selection = active?.tagName === 'INPUT' ? [active.selectionStart, active.selectionEnd] : undefined;
        this.contentNode.replaceChildren();
        this.contentNode.setAttribute('aria-busy', String(this.busy || this.loading));
        this.closeButton.disabled = this.busy;
        this.closeCrossNode.setAttribute('aria-disabled', String(this.busy));
        this.contentNode.appendChild(this.element('p', 'paddock-account-intro', 'Start a new conversation with any account. Your existing terminals stay open.'));
        if (this.loading) {
            const status = this.element('p', 'paddock-account-empty', 'Loading accounts…');
            status.setAttribute('role', 'status');
            this.contentNode.appendChild(status);
        } else {
            const list = this.element('ul', 'paddock-account-list');
            list.setAttribute('aria-label', 'Saved accounts');
            for (const profile of this.profiles) {
                list.appendChild(this.renderProfile(profile));
            }
            this.contentNode.appendChild(list);
            if (!this.profiles.length && !this.error) {
                this.contentNode.appendChild(this.element('p', 'paddock-account-empty', 'No accounts added yet.'));
            }
            if (this.formVisible) {
                this.contentNode.appendChild(this.renderForm());
            } else if (!this.loadFailed) {
                this.contentNode.appendChild(this.button('Add account', 'add-account', () => {
                    this.formVisible = true;
                    this.draft.label = this.suggestLabel();
                    this.render();
                    this.focus('account-name');
                    const form = this.contentNode.querySelector('.paddock-account-form');
                    // Keep the submit and cancel actions visible when the whole form fits below a long account list.
                    if (form && form.offsetHeight <= this.contentNode.clientHeight) {
                        form.scrollIntoView({ block: 'nearest' });
                    }
                }));
            }
        }
        if (this.error) {
            const error = this.element('div', 'paddock-account-error');
            error.setAttribute('role', 'alert');
            error.appendChild(this.element('p', '', this.error));
            if (this.loadFailed) {
                error.appendChild(this.button('Retry', 'retry-load', () => void this.load()));
            }
            this.contentNode.appendChild(error);
        }
        if (focusKey) {
            const focused = this.focus(focusKey);
            if (focused?.tagName === 'INPUT' && selection) {
                focused.setSelectionRange(...selection);
            }
        }
    }

    focus(
        key,
    ) {
        const node = Array.from(this.contentNode.querySelectorAll('[data-focus]')).find(item => item.dataset.focus === key);
        node?.focus();
        return node;
    }

    renderProfile(
        profile,
    ) {
        const row = this.element('li', 'paddock-account-row');
        row.dataset.accountId = profile.id;
        const heading = this.element('div', 'paddock-account-summary');
        const title = `${AccountLabels.PROVIDERS[profile.provider]} · ${profile.label}`;
        const name = this.element('strong', 'paddock-account-name', title);
        name.title = title;
        let runtime = this.options.runtimeOptions?.find(option => option.value === profile.runtime)?.label || AccountLabels.RUNTIMES[profile.runtime];
        if (profile.runtime === 'wsl' && profile.wslDistribution) {
            runtime += ` · ${profile.wslDistribution}`;
        }
        heading.append(name, this.element('span', 'paddock-account-detail', runtime));
        row.appendChild(heading);
        if (this.editing?.id === profile.id) {
            const form = this.element('form', 'paddock-account-row-form');
            const input = this.input(this.editing.label, `rename-${profile.id}`, value => { this.editing.label = value; });
            const save = this.button(this.busy ? 'Saving…' : 'Save', `save-${profile.id}`, () => {}, true);
            save.type = 'submit';
            form.addEventListener('submit', event => {
                event.preventDefault();
                void this.rename(profile);
            });
            form.append(this.field('Account name', input), save, this.button('Cancel', `cancel-${profile.id}`, () => {
                this.editing = undefined;
                this.error = '';
                this.render();
                this.focus(`edit-${profile.id}`);
            }));
            row.appendChild(form);
        } else if (this.removing === profile.id) {
            const confirmation = this.element('div', 'paddock-account-remove');
            confirmation.appendChild(this.element('p', '', 'Remove this account from the list? Existing terminals, login data, and conversations will stay on this device.'));
            confirmation.append(this.button(this.busy ? 'Removing…' : 'Remove', `confirm-remove-${profile.id}`, () => void this.remove(profile)), this.button('Cancel', `cancel-remove-${profile.id}`, () => {
                this.removing = undefined;
                this.error = '';
                this.render();
                this.focus(`remove-${profile.id}`);
            }));
            row.appendChild(confirmation);
        } else {
            const actions = this.element('div', 'paddock-account-actions');
            const open = this.button(this.busyId === profile.id ? 'Opening…' : 'Open', `open-${profile.id}`, () => void this.openProfile(profile), true);
            open.setAttribute('aria-label', `Open ${title} in a new terminal`);
            const rename = this.button('Rename', `edit-${profile.id}`, () => {
                this.editing = { id: profile.id, label: profile.label };
                this.removing = undefined;
                this.error = '';
                this.render();
                this.focus(`rename-${profile.id}`)?.select();
            });
            rename.setAttribute('aria-label', `Rename ${title}`);
            const remove = this.button('Remove', `remove-${profile.id}`, () => {
                this.removing = profile.id;
                this.editing = undefined;
                this.error = '';
                this.render();
                this.focus(`cancel-remove-${profile.id}`);
            });
            remove.setAttribute('aria-label', `Remove ${title}`);
            actions.append(open, rename, remove);
            row.appendChild(actions);
        }
        return row;
    }

    renderForm() {
        const form = this.element('form', 'paddock-account-form');
        form.setAttribute('aria-label', 'Add account');
        form.appendChild(this.element('h3', '', 'Add account'));
        const fields = this.element('div', 'paddock-account-fields');
        const provider = this.select(Object.entries(AccountLabels.PROVIDERS).map(([value, label]) => ({ value, label })), this.draft.provider, 'provider', value => {
            const wasSuggested = this.draft.label === this.suggestLabel();
            this.draft.provider = value;
            if (wasSuggested) {
                this.draft.label = this.suggestLabel();
            }
            this.render();
        });
        const name = this.input(this.draft.label, 'account-name', value => { this.draft.label = value; });
        name.placeholder = 'Main';
        fields.append(this.field('Tool', provider), this.field('Account name', name));
        if (this.options.runtimeOptions?.length > 1) {
            fields.appendChild(this.field('Run in', this.select(this.options.runtimeOptions, this.draft.runtime, 'runtime', value => { this.draft.runtime = value; })));
        }
        form.appendChild(fields);
        form.appendChild(this.element('p', 'paddock-account-hint', 'Your existing skills are shared automatically. Sign in through Claude or Codex in the new terminal.'));
        const actions = this.element('div', 'paddock-account-form-actions');
        const add = this.button(this.creating ? 'Adding…' : 'Add & Open', 'create-account', () => {}, true);
        add.type = 'submit';
        actions.appendChild(add);
        if (this.profiles.length) {
            actions.appendChild(this.button('Cancel', 'cancel-add', () => {
                this.formVisible = false;
                this.error = '';
                this.render();
                this.focus('add-account');
            }));
        }
        form.appendChild(actions);
        form.addEventListener('submit', event => {
            event.preventDefault();
            void this.create();
        });
        return form;
    }

    async load() {
        this.loading = true;
        this.loadFailed = false;
        this.error = '';
        this.render();
        try {
            this.profiles = await retryAccountStorage(() => this.options.service.list(), this.options.service, {});
            this.formVisible = this.profiles.length === 0;
            this.draft.label = this.suggestLabel();
        } catch {
            this.loadFailed = true;
            this.error = 'Could not load your accounts. Try again.';
        } finally {
            this.loading = false;
            if (!this.isDisposed) {
                this.render();
                this.focus(this.loadFailed ? 'retry-load' : this.profiles.length ? `open-${this.profiles[0].id}` : 'account-name');
            }
        }
    }

    async create() {
        if (!this.busy && !this.loading) {
            const label = this.draft.label.trim();
            const error = this.labelError(label, this.draft.provider);
            if (error) {
                this.error = error;
                this.render();
                this.focus('account-name');
            } else {
                this.busy = true;
                this.creating = true;
                this.error = '';
                this.render();
                let profile;
                try {
                    const draft = { ...this.draft, label };
                    profile = await retryAccountStorage(() => this.options.service.create(draft), this.options.service, { runtime: draft.runtime });
                    this.profiles.push(profile);
                    this.formVisible = false;
                } catch {
                    this.error = 'Could not add this account. Check that its environment and skills folders are available, then try again.';
                } finally {
                    this.creating = false;
                    this.busy = false;
                    this.render();
                }
                if (profile) {
                    await this.openProfile(profile);
                } else {
                    this.focus('create-account');
                }
            }
        }
    }

    async openProfile(
        profile,
    ) {
        if (!this.busy && !this.loading) {
            this.busy = true;
            this.busyId = profile.id;
            this.error = '';
            this.render();
            let opened = false;
            try {
                await this.options.onOpen(profile);
                opened = true;
            } catch {
                this.error = `Could not open ${AccountLabels.PROVIDERS[profile.provider]} · ${profile.label}. Check that the selected environment and CLI are available, then try Open again.`;
            } finally {
                this.busy = false;
                this.busyId = undefined;
                this.render();
            }
            if (opened) {
                this.close();
            } else {
                this.focus(`open-${profile.id}`);
            }
        }
    }

    async rename(
        profile,
    ) {
        if (!this.busy && this.editing?.id === profile.id) {
            const label = this.editing.label.trim();
            const error = this.labelError(label, profile.provider, profile.id);
            if (error) {
                this.error = error;
                this.render();
                this.focus(`rename-${profile.id}`);
            } else {
                this.busy = true;
                this.error = '';
                this.render();
                try {
                    const updated = await this.options.service.rename(profile.id, label);
                    this.profiles = this.profiles.map(item => item.id === profile.id ? updated : item);
                    this.editing = undefined;
                } catch {
                    this.error = 'Could not rename this account. Try again.';
                } finally {
                    this.busy = false;
                    this.render();
                    this.focus(this.editing ? `rename-${profile.id}` : `edit-${profile.id}`);
                }
            }
        }
    }

    async remove(
        profile,
    ) {
        if (!this.busy) {
            this.busy = true;
            this.error = '';
            this.render();
            try {
                await this.options.service.remove(profile.id);
                this.profiles = this.profiles.filter(item => item.id !== profile.id);
                this.removing = undefined;
                this.formVisible = this.profiles.length === 0;
                this.draft.label = this.suggestLabel();
            } catch {
                this.error = 'Could not remove this account. Try again.';
            } finally {
                this.busy = false;
                this.render();
                this.focus(this.removing ? `cancel-remove-${profile.id}` : this.profiles.length ? 'add-account' : 'account-name');
            }
        }
    }
}

module.exports = { AccountDialog };
