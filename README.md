# Herdr Terminal

터미널과 프로젝트 파일을 한 창에서 다루는 macOS·Windows 데스크톱 앱입니다.

## 다운로드

최신 릴리스의 설치 파일을 받습니다. 링크는 새 릴리스가 나오면 자동으로 그 파일을 가리킵니다.

- [macOS (Apple Silicon) — Herdr-Terminal-mac-arm64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Herdr-Terminal-mac-arm64.dmg)
- [macOS (Intel) — Herdr-Terminal-mac-x64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Herdr-Terminal-mac-x64.dmg)
- [Windows — Herdr-Terminal-win-x64.exe](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Herdr-Terminal-win-x64.exe)

지난 버전은 [릴리스 목록](https://github.com/YoungJaeChoung/paddock/releases)에 있습니다. 저장소가 비공개인 동안에는 저장소 접근 권한이 있는 GitHub 계정으로 로그인해야 받을 수 있습니다.

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

- **작업 시작:** 앱을 켜면 첫 터미널이 열린 폴더(보통 홈 `~`)가 바로 작업 목록에 서고, 그 터미널이 첫 작업 터미널입니다. 거기서 `git clone`하거나 `cd`로 레포 폴더에 들어간 뒤, 사이드바 **New work terminal**을 누릅니다. 지금 터미널의 폴더가 작업 목록에 생기고 그 아래에 작업 터미널이 붙습니다. 폴더를 고르는 창은 없습니다.
- **여러 레포 오가기:** 사이드바 **Work**에 작업 폴더와 그 아래 작업 터미널이 트리로 보입니다. 터미널을 누르면 전환되고, 두 번 누르면 이름을 바꿉니다. 오른쪽 흐린 글자는 실행 중인 프로그램입니다. 아래 구획에는 선택한 폴더의 파일이 나옵니다.
- **정리:** 터미널 줄의 `×`로 터미널 하나를 닫습니다. 폴더 줄의 `⋯` → **Remove from list**는 그 폴더의 터미널을 닫고 목록에서 뺍니다. 디스크의 폴더는 지우지 않습니다.
- **추가 터미널:** 위쪽 줄의 `＋`는 작업 폴더와 무관한 터미널을 엽니다. `⌄`에서 셸을 고릅니다(macOS zsh·bash, Windows PowerShell·명령 프롬프트·Git Bash·WSL).
- **나누기:** `` Ctrl+Shift+` ``는 같은 폴더의 새 작업 터미널을 아래 칸에, `Ctrl+Shift+5`(macOS `⌘\`)는 옆 칸에 엽니다. 각 칸 위 경로 줄의 버튼으로도 됩니다.
- **복사:** 터미널 글자를 드래그해 선택하면 바로 복사됩니다.
- **에이전트 완료 알림:** 보고 있지 않은 터미널에서 Claude·Codex 같은 에이전트가 5초 넘게 일하다 멈추면(끝났거나 답을 기다림) 짧은 소리가 나고, 사이드바·탭에 초록 점이 붙습니다. 그 터미널을 열면 점이 사라집니다. 소리는 설정(Preferences)의 `herdr.agentDoneSound`로 끕니다. 사이드바 오른쪽 흐린 글자는 그 터미널에서 지금 실행 중인 프로그램입니다(Windows는 셸 이름).
- **Source control·Extensions:** 사이드바 위 아이콘으로 전환합니다. Source control은 작업 폴더의 변경·커밋을, Extensions는 VS Code처럼 Open VSX에서 확장을 찾아 설치합니다. `.vsix` 파일은 확장 보기로 끌어다 놓습니다.
- **원격:** 상태 줄 맨 왼쪽 원격 표시를 눌러 SSH 호스트에 연결합니다. 연결되면 창 전체가 그 호스트에서 동작합니다. 오른쪽 **Memory**는 시스템 메모리 사용률입니다.
- **AI 사용량:** 상태 줄 오른쪽에 Claude·Codex 사용 한도가 나옵니다. 출처마다 표지(✱ Claude · ◎ Codex · ▦ Memory)와 이름을 한 번 쓰고, 그 뒤에 `5h`(5시간 창)·`wk`(주간 창) 게이지와 %가 붙습니다. 막대에 마우스를 올리면 남은 양과 초기화까지 남은 시간이 보입니다. 로그인은 필요 없습니다 — Codex는 세션 기록을, Claude는 Claude Code 상태 줄 입력(Pro·Max, 첫 응답 뒤)을 읽습니다. Claude 표시를 위해 첫 실행에 `~/.claude/settings.json`에 Herdr 상태 줄을 넣고(원래 상태 줄은 이어서 보여 주고, 원본은 `settings.json.herdr-backup`), Claude 묶음을 눌러 끄면 원래대로 되돌립니다. 상태 줄 실행에 Node.js가 필요합니다(macOS·Linux는 없으면 앱 내장 런타임을 씁니다).
- **저장:** 하단 **Save** 또는 `⌘S`(Windows는 `Ctrl+S`)로 저장합니다. 저장하지 않고 닫으면 편집기의 저장 확인이 나타납니다.

기본 터미널 사용은 로컬에서 실행됩니다. 앱 자체가 외부로 보내는 것은 확장 검색어(Open VSX)뿐입니다. AI 사용량은 로컬 파일만 읽습니다. 사용자가 실행한 명령과 설치한 확장은 외부 서비스에 연결하거나 로컬 파일에 접근할 수 있습니다.

## 현재 범위

버전 0.1.0입니다. 로컬 터미널, 텍스트 편집·저장, VSIX 설치와 전용 편집기 연결을 검증했습니다. pen.dev는 자체 로그인을 요구하며 로그인 이후 도형 편집·저장은 미검증입니다. 모든 VS Code 확장이 호환되는 것은 아닙니다.

개발자 인증서로 서명한 설치 파일, 앱 아이콘, 자동 업데이트, 확장 제거·업데이트 화면은 아직 제공하지 않습니다. 무료 오픈소스 공개를 목표로 하며 공개 배포 전 제품 라이선스를 확정해야 합니다.

## 개발

제품 코드는 `desktop/`에 있습니다. 제품 빌드는 `prototype/`에 의존하지 않습니다. `prototype/`은 초기 HTML 시안과 이전 구현을 보관한 자료입니다. 저장소에서는 `.pen` 디자인 파일을 만들지 않습니다.

제품을 사용하며 피드백을 남기려면 `make feedback`을 실행하세요. 실행한 운영체제용 앱이 뜨며, 처음이면 setup·build를 먼저 자동으로 합니다. 우하단 메모 도구를 켜고 요소를 클릭하거나 빈 공간을 드래그한 다음, 한 줄을 적고 Enter를 누릅니다. 메모와 당시 화면·선택 부분 이미지는 `out/memo/<날짜>/`에 저장됩니다. `make feedback-product`도 같은 명령이고, `make`가 없는 Windows에서는 `npm run feedback`을 씁니다. 평소 `npm start`에는 메모 도구가 나타나지 않습니다.

피드백 화면의 Agentation은 개발용 의존성(PolyForm Shield 1.0.0)이며 일반 제품 화면에는 로드되지 않습니다.

