const { open, OpenerService } = require('@theia/core/lib/browser/opener-service');
const { MessageService } = require('@theia/core/lib/common/message-service');
const { ApplicationServer } = require('@theia/core/lib/common/application-protocol');
const { VSCodeExtensionUri } = require('@theia/plugin-ext-vscode/lib/common/plugin-vscode-uri');
const { VSXExtensionsModel } = require('@theia/vsx-registry/lib/browser/vsx-extensions-model');
const { VSXRegistryService } = require('@theia/vsx-registry/lib/common/vsx-registry-service');
const { compareVersions } = require('./extension-version');

/**
 * VS Code의 `extension.open` 명령을 Paddock(Theia)에 제공한다.
 *
 * VS Code 확장은 "새 버전이 있다" 같은 알림의 버튼에서 `extension.open`으로 자기 확장 페이지를 연다. Theia에는 이 명령이 없어
 * 버튼을 눌러도 알림만 닫혔다. 여기서는 그 확장의 페이지를 열고, Open VSX에 설치본보다 새 버전이 있으면 바로 업데이트할지 묻는다.
 * 인자가 확장 id 문자열이 아니면 아무것도 하지 않는다.
 */
class PaddockExtensionOpen {
    constructor(
        container,
    ) {
        this.container = container;
    }

    registerCommands(
        registry,
    ) {
        registry.registerCommand({ id: 'extension.open' }, { execute: id => this.open(id) });
    }

    async open(
        id,
    ) {
        if (typeof id === 'string' && id !== '') {
            await open(this.container.get(OpenerService), VSCodeExtensionUri.fromId(id));
            await this.offerUpdate(id);
        }
    }

    /** 설치본보다 새 호환 버전이 Open VSX에 있으면 업데이트를 묻고, 고르면 그 판을 설치하고 창 다시 불러오기를 권한다. */
    async offerUpdate(
        id,
    ) {
        const model = this.container.get(VSXExtensionsModel);
        const extension = model.isInstalled(id) ? await model.resolve(id).catch(() => model.getExtension(id)) : undefined;
        const installed = extension?.installedVersion ?? extension?.version;
        const latest = installed && await this.container.get(VSXRegistryService).findLatestCompatibleExtension({
            extensionId: id,
            includeAllVersions: true,
            targetPlatform: await this.container.get(ApplicationServer).getApplicationPlatform(),
        }).catch(() => undefined);
        if (latest && compareVersions(latest.version, installed) > 0) {
            const name = extension.displayName || id;
            const choice = await this.container.get(MessageService).info(`${name} ${latest.version} is available. Installed: ${installed}.`, 'Update');
            if (choice === 'Update') {
                // 설치가 끝나도 실행 중인 확장과 서버의 설치 목록은 창을 다시 부르기 전까지 옛 판을 가리킨다. 그래서 오류 없이 끝났는지만으로 가른다.
                const failed = await extension.install({ version: latest.version, ignoreOtherVersions: true }).then(() => false, () => true);
                const messages = this.container.get(MessageService);
                if (failed) messages.error(`Could not update ${name} to ${latest.version}. Try again from its page in Extensions.`);
                else if (await messages.info(`Reload the window to use ${name} ${latest.version}.`, 'Reload Window') === 'Reload Window') extension.reloadWindow();
            }
        }
    }
}

module.exports = { PaddockExtensionOpen };
