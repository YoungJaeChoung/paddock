Object.defineProperty(exports, "__esModule", { value: true });
// Herdr 창을 실행 중인 OS에 맞추고, 백엔드가 뜨기 전에 확장·설정 폴더를 정한다.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { ContainerModule, injectable, decorate } = require('@theia/core/shared/inversify');
const { ElectronMainApplication } = require('@theia/core/lib/electron-main/electron-main-application');
const { isOSX } = require('@theia/core/lib/common/os');

// macOS에서만 의미 있는 창 옵션. Windows에서 titleBarStyle을 넘기면 창 틀과 닫기 버튼이 사라진다.
const MAC_ONLY_WINDOW_OPTIONS = ['titleBarStyle', 'trafficLightPosition'];

/**
 * 사용자 확장·설정 폴더를 환경 변수로 지정한다.
 *
 * 개발 실행(`npm start`)과 설치본이 같은 폴더를 쓰도록 메인 프로세스에서 정한다.
 * 백엔드 프로세스는 이 환경 변수를 물려받는다. `HERDR_EXTENSIONS_DIR`·`HERDR_CONFIG_DIR`가 있으면
 * 그 경로를, 없으면 `~/.herdr/extensions`·`~/.herdr/config`를 만들어 쓴다.
 */
function useHerdrDirectories() {
    const extensionDirectory = path.resolve(process.env.HERDR_EXTENSIONS_DIR || path.join(os.homedir(), '.herdr', 'extensions'));
    const configDirectory = path.resolve(process.env.HERDR_CONFIG_DIR || path.join(os.homedir(), '.herdr', 'config'));
    fs.mkdirSync(extensionDirectory, { recursive: true });
    fs.mkdirSync(configDirectory, { recursive: true });
    process.env.THEIA_CONFIG_DIR = configDirectory;
    // Windows 경로(C:\...)도 확장 목록 주소로 읽히도록 /C:/... 형태로 바꿔 넘긴다.
    const extensionEntry = `local-dir:${pathToFileURL(extensionDirectory).pathname}`;
    process.env.THEIA_DEFAULT_PLUGINS = [process.env.THEIA_DEFAULT_PLUGINS, extensionEntry].filter(Boolean).join(',');
}

/** Herdr 설정을 적용한 뒤 Theia 데스크톱 앱을 시작한다. */
class HerdrMainApplication extends ElectronMainApplication {
    async start(
        config,
    ) {
        useHerdrDirectories();
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
decorate(injectable(), HerdrMainApplication);

exports.default = new ContainerModule((bind, unbind, isBound, rebind) => {
    rebind(ElectronMainApplication).to(HerdrMainApplication).inSingletonScope();
});
