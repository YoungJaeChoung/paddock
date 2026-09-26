# Herdr Terminal

터미널과 프로젝트 파일을 한 창에서 다루는 macOS·Windows 데스크톱 앱입니다.

## 다운로드

최신 릴리스의 설치 파일을 받습니다. 링크는 새 릴리스가 나오면 자동으로 그 파일을 가리킵니다.

- [macOS (Apple Silicon) — Herdr-Terminal-mac-arm64.dmg](https://github.com/YoungJaeChoung/herdr-terminal/releases/latest/download/Herdr-Terminal-mac-arm64.dmg)
- [macOS (Intel) — Herdr-Terminal-mac-x64.dmg](https://github.com/YoungJaeChoung/herdr-terminal/releases/latest/download/Herdr-Terminal-mac-x64.dmg)
- [Windows — Herdr-Terminal-win-x64.exe](https://github.com/YoungJaeChoung/herdr-terminal/releases/latest/download/Herdr-Terminal-win-x64.exe)

지난 버전은 [릴리스 목록](https://github.com/YoungJaeChoung/herdr-terminal/releases)에 있습니다. 저장소가 비공개인 동안에는 저장소 접근 권한이 있는 GitHub 계정으로 로그인해야 받을 수 있습니다.

처음 실행할 때 macOS는 앱을 Control-클릭해 **열기**를 고르고, Windows는 SmartScreen 창에서 **추가 정보 → 실행**을 누릅니다. 설치 파일에 개발자 인증서 서명이 없기 때문입니다.

## 실행

Node.js 22 이상과 네이티브 모듈을 컴파일할 개발 도구가 필요합니다. macOS는 Xcode Command Line Tools(`xcode-select --install`), Windows는 Visual Studio Build Tools의 "C++를 사용한 데스크톱 개발"과 Python 3입니다. 저장소 루트에서 실행하세요.

```bash
npm run setup
npm run build
npm start
```

프로젝트를 지정해서 열 수도 있습니다.

```bash
npm start -- /프로젝트/절대경로
```

## 설치 파일 만들기

설치 파일은 그 운영체제에서 만듭니다. 터미널 기능의 네이티브 모듈을 운영체제별로 컴파일하기 때문입니다. 결과는 `desktop/dist/`에 생깁니다.

| 운영체제 | 명령 | 결과 |
| --- | --- | --- |
| macOS | `make package-mac` | `Herdr-Terminal-mac-<arm64 또는 x64>.dmg` |
| Windows | `make package-win` | `Herdr-Terminal-win-x64.exe` (설치 마법사) |

Windows에 `make`가 없으면 `npm run setup` 다음 `npm run package:win`을 실행해도 같습니다.

한쪽 운영체제만 있으면 GitHub Actions의 **Package** 작업으로 맥(Apple Silicon·Intel)·Windows 설치 파일을 모두 만듭니다. 로컬 `make package-mac`은 그 맥의 CPU용 하나만 만듭니다.

| 올리는 것 | 결과 |
| --- | --- |
| `package/`로 시작하는 브랜치 | 실행 화면의 Artifacts에 설치 파일 (확인용) |
| `make release` (`v<버전>` 태그) | GitHub 릴리스 생성. 위 다운로드 링크가 이 파일로 바뀜 |

`make release`는 `desktop/package.json`의 버전으로 태그를 만듭니다. 새 버전을 내려면 먼저 그 버전을 올리고 커밋하세요. 설치본은 개발 실행과 같은 `~/.herdr/extensions`·`~/.herdr/config`를 씁니다.

## 사용

- **터미널:** 상단 `＋`로 시작합니다. `⌄`에서 등록된 셸을 선택합니다. macOS는 zsh·bash, Windows는 PowerShell(기본)·명령 프롬프트·Git Bash·WSL입니다. 설치되지 않은 셸을 고르면 실행 파일 경로 오류가 표시됩니다. 탭마다 독립된 셸을 사용하며 종료 전 확인합니다.
- **프로젝트:** 왼쪽 파일 메뉴에서 폴더를 엽니다. 파일을 클릭하면 같은 창의 탭에서 편집합니다. 현재 파일은 목록에 강조됩니다.
- **저장:** 변경된 탭에는 `●`가 표시됩니다. 하단 **저장** 또는 `⌘S`(Windows는 `Ctrl+S`)로 저장합니다. 저장하지 않고 닫으면 편집기의 저장 확인이 나타납니다.
- **확장:** 왼쪽 확장 메뉴에서 `.vsix`를 설치합니다. 설치 자체에는 계정이 필요하지 않습니다. 확장이 제공하는 편집 화면은 Herdr 파일 탭에 표시됩니다.

기본 터미널 사용은 로컬에서 실행됩니다. 사용자가 실행한 명령과 설치한 확장은 외부 서비스에 연결하거나 로컬 파일에 접근할 수 있습니다.

## 현재 범위

버전 0.1.0입니다. 로컬 터미널, 텍스트 편집·저장, VSIX 설치와 전용 편집기 연결을 검증했습니다. pen.dev는 자체 로그인을 요구하며 로그인 이후 도형 편집·저장은 미검증입니다. 모든 VS Code 확장이 호환되는 것은 아닙니다.

개발자 인증서로 서명한 설치 파일, 앱 아이콘, 자동 업데이트, 확장 제거·업데이트 화면은 아직 제공하지 않습니다. 무료 오픈소스 공개를 목표로 하며 공개 배포 전 제품 라이선스를 확정해야 합니다.

## 개발

제품 코드는 `desktop/`에 있습니다. 제품 빌드는 `prototype/`에 의존하지 않습니다. `prototype/`은 초기 HTML 시안과 이전 구현을 보관한 자료입니다. 저장소에서는 `.pen` 디자인 파일을 만들지 않습니다.

제품을 사용하며 피드백을 남기려면 `make feedback`을 실행하세요. 실행한 운영체제용 앱이 뜨며, 처음이면 setup·build를 먼저 자동으로 합니다. 우하단 메모 도구를 켜고 요소를 클릭하거나 빈 공간을 드래그한 다음, 한 줄을 적고 Enter를 누릅니다. 메모와 당시 화면·선택 부분 이미지는 `out/memo/<날짜>/`에 저장됩니다. `make feedback-product`도 같은 명령이고, `make`가 없는 Windows에서는 `npm run feedback`을 씁니다. 평소 `npm start`에는 메모 도구가 나타나지 않습니다.

피드백 화면의 Agentation은 개발용 의존성(PolyForm Shield 1.0.0)이며 일반 제품 화면에는 로드되지 않습니다.

