Object.defineProperty(exports, "__esModule", { value: true });
// Paddock 창을 실행 중인 OS에 맞추고, 백엔드가 뜨기 전에 확장·설정 폴더를 정한다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMainApplication } = require('@theia/core/lib/electron-main/electron-main-application');
const { isOSX } = require('@theia/core/lib/common/os');
const { moveLegacyDirectory } = require('./legacy-directory');

// macOS에서만 의미 있는 창 옵션. Windows에서 titleBarStyle을 넘기면 창 틀과 닫기 버튼이 사라진다.
const MAC_ONLY_WINDOW_OPTIONS = ['titleBarStyle', 'trafficLightPosition'];

/**
 * 사용자 확장·설정 폴더와 앱에 동봉한 기본 확장 폴더를 환경 변수로 지정한다.
 *
 * 개발 실행(`npm start`)과 설치본이 같은 폴더를 쓰도록 메인 프로세스에서 정한다.
 * 백엔드 프로세스는 이 환경 변수를 물려받는다. `PADDOCK_EXTENSIONS_DIR`·`PADDOCK_CONFIG_DIR`가 있으면
 * 그 경로를, 없으면 `~/.paddock/extensions`·`~/.paddock/config`를 만들어 쓴다.
 * 이전 이름으로 쓰던 `~/.herdr`가 있으면 기본 폴더를 만들기 전에 그 폴더를 옮겨 이어 쓴다.
 * 앱 폴더의 `plugins`(Source control 보기에 쓰는 VS Code 내장 git 확장)는 있을 때만 더한다.
 */
function usePaddockDirectories() {
    if (!process.env.PADDOCK_EXTENSIONS_DIR && !process.env.PADDOCK_CONFIG_DIR) {
        const claudeSettingsPath = path.join(process.env.CLAUDE_CONFIG_DIR || path.join(os.homedir(), '.claude'), 'settings.json');
        try {
            moveLegacyDirectory(os.homedir(), path.join(os.homedir(), '.paddock'), claudeSettingsPath);
        } catch (error) {
            // 옮기지 못해도 앱은 새 폴더로 시작한다. 옛 폴더는 그대로 남아 손으로 옮길 수 있다.
            console.warn('Could not move ~/.herdr to ~/.paddock:', error);
        }
    }
    const extensionDirectory = path.resolve(process.env.PADDOCK_EXTENSIONS_DIR || path.join(os.homedir(), '.paddock', 'extensions'));
    const configDirectory = path.resolve(process.env.PADDOCK_CONFIG_DIR || path.join(os.homedir(), '.paddock', 'config'));
    fs.mkdirSync(extensionDirectory, { recursive: true });
    fs.mkdirSync(configDirectory, { recursive: true });
    process.env.THEIA_CONFIG_DIR = configDirectory;
    // Windows 경로(C:\...)도 확장 목록 주소로 읽히도록 /C:/... 형태로 바꿔 넘긴다.
    const extensionEntry = `local-dir:${pathToFileURL(extensionDirectory).pathname}`;
    const bundledDirectory = path.join(process.env.THEIA_APP_PROJECT_PATH || process.cwd(), 'plugins');
    const bundledEntry = fs.existsSync(bundledDirectory) ? `local-dir:${pathToFileURL(bundledDirectory).pathname}` : undefined;
    process.env.THEIA_DEFAULT_PLUGINS = [process.env.THEIA_DEFAULT_PLUGINS, bundledEntry, extensionEntry].filter(Boolean).join(',');
}

/** Paddock 설정을 적용한 뒤 Theia 데스크톱 앱을 시작한다. */
class PaddockMainApplication extends ElectronMainApplication {
    async start(
        config,
    ) {
        usePaddockDirectories();
        await super.start(config);
    }

    getDefaultOptions() {
        const options = super.getDefaultOptions();
        if (!isOSX) {
            for (const key of MAC_ONLY_WINDOW_OPTIONS) {
                delete options[key];
            }
        }
        return options;
    }
}
decorate(injectable(), PaddockMainApplication);

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMainApplication).to(PaddockMainApplication).inSingletonScope();
});
