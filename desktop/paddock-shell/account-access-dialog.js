const { AbstractDialog } = require('@theia/core/lib/browser/dialogs');

/** Keeps the failed action pending while the user restores account storage access. */
class AccountAccessDialog extends AbstractDialog {
    constructor(
        service,
        request,
        access,
        action,
    ) {
        super({ title: 'Account access', maxWidth: 540 });
        this.service = service;
        this.request = request;
        this.access = access;
        this.action = action;
        this.busy = false;
        this.completed = undefined;
        this.addClass('paddock-account-access-dialog');
        this.titleNode.id = 'paddock-account-access-title';
        const block = this.node.querySelector('.dialogBlock');
        block.setAttribute('role', 'dialog');
        block.setAttribute('aria-modal', 'true');
        block.setAttribute('aria-labelledby', this.titleNode.id);
        this.closeCrossNode.setAttribute('role', 'button');
        this.closeCrossNode.setAttribute('aria-label', 'Cancel account access');
        this.closeCrossNode.tabIndex = 0;
        this.closeCrossNode.addEventListener('keydown', event => {
            if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                this.close();
            }
        });
        this.appendCloseButton('Cancel');
        this.render();
    }

    get value() {
        return this.completed;
    }

    handleEnter() {
        // Permission changes require the focused button's own action, never an implicit dialog accept.
        return false;
    }

    onActivateRequest(
        message,
    ) {
        super.onActivateRequest(message);
        this.closeButton.focus();
    }

    close() {
        if (!this.busy) super.close();
    }

    render() {
        const document = this.node.ownerDocument;
        const message = document.createElement('p');
        message.textContent = this.access.message;
        this.contentNode.replaceChildren(message);
        if (this.access.folder) {
            const folder = document.createElement('p');
            folder.textContent = this.access.folder;
            folder.style.overflowWrap = 'anywhere';
            this.contentNode.appendChild(folder);
        }
        this.contentNode.setAttribute('aria-busy', String(this.busy));
        if (this.error) {
            const error = document.createElement('p');
            error.setAttribute('role', 'alert');
            error.textContent = this.error;
            this.contentNode.appendChild(error);
        }
        const actions = document.createElement('div');
        actions.className = 'paddock-account-form-actions';
        const addButton = (text, action, primary = false) => {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = `theia-button ${primary ? 'main' : 'secondary'}`;
            button.textContent = text;
            button.disabled = this.busy;
            button.addEventListener('click', action);
            actions.appendChild(button);
        };
        if (this.access.status === 'repair') addButton('Allow access', () => void this.retry(true), true);
        if (this.access.canOpenSettings) addButton(this.access.settingsLabel, () => void this.openSettings());
        addButton(this.busy ? 'Retrying…' : 'Retry', () => void this.retry(false), this.access.status !== 'repair');
        this.contentNode.appendChild(actions);
        this.closeButton.disabled = this.busy;
        this.closeCrossNode.setAttribute('aria-disabled', String(this.busy));
    }

    async openSettings() {
        if (!this.busy) {
            this.busy = true;
            this.error = '';
            this.render();
            try {
                await this.service.openStorageSettings(this.request);
            } catch {
                this.error = 'Folder settings could not be opened. Check permissions in your file manager, then choose Retry.';
            } finally {
                this.busy = false;
                this.render();
            }
        }
    }

    async retry(
        allow,
    ) {
        if (!this.busy) {
            this.busy = true;
            this.error = '';
            this.render();
            try {
                if (allow) await this.service.allowStorageAccess(this.request);
                const value = await this.action();
                this.completed = { value };
            } catch (error) {
                this.failure = error;
                const access = await this.service.inspectStorageAccess(this.request).catch(() => null);
                if (access) this.access = access.status === 'ready' ? { ...access, message: 'Folder access is available.' } : access;
                else this.access = { status: 'unknown', message: 'Paddock could not check folder access. Retry the account action, or cancel to return.', canOpenSettings: false };
                this.error = !access || access.status === 'ready'
                    ? (error?.message || 'The account action still failed. Check that the environment and CLI are available, or cancel to return to your accounts.')
                    : 'Access is still blocked. Review the folder permissions, then try again. Your existing accounts and terminals have been kept.';
            } finally {
                this.busy = false;
                this.render();
            }
            if (this.completed) await this.accept();
        }
    }
}

/** Unrelated CLI or working-folder errors keep their original handling; only confirmed storage failures open this dialog. */
async function retryAccountStorage(
    action,
    service,
    request,
) {
    let result;
    try {
        result = await action();
    } catch (error) {
        const access = await service.inspectStorageAccess(request).catch(() => null);
        if (!access || access.status === 'ready') throw error;
        const dialog = new AccountAccessDialog(service, request, access, action);
        const completed = await dialog.open();
        if (!completed) throw dialog.failure || error;
        result = completed.value;
    }
    return result;
}

module.exports = { AccountAccessDialog, retryAccountStorage };
