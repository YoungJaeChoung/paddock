# Paddock

터미널과 프로젝트 파일을 한 창에서 다루는 macOS·Windows 데스크톱 앱입니다.

## 다운로드

최신 릴리스의 설치 파일을 받습니다. 링크는 새 릴리스가 나오면 자동으로 그 파일을 가리킵니다.

- [macOS (Apple Silicon) — Paddock-mac-arm64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-arm64.dmg)
- [macOS (Intel) — Paddock-mac-x64.dmg](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-mac-x64.dmg)
- [Windows — Paddock-win-x64.exe](https://github.com/YoungJaeChoung/paddock/releases/latest/download/Paddock-win-x64.exe)

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
| macOS | `make package-mac` | `Paddock-mac-<arm64 또는 x64>.dmg` |
| Windows | `make package-win` | `Paddock-win-x64.exe` (설치 마법사) |

Windows에 `make`가 없으면 `npm run setup` 다음 `npm run package:win`을 실행해도 같습니다.

한쪽 운영체제만 있으면 GitHub Actions의 **Package** 작업으로 맥(Apple Silicon·Intel)·Windows 설치 파일을 모두 만듭니다. 로컬 `make package-mac`은 그 맥의 CPU용 하나만 만듭니다.

| 올리는 것 | 결과 |
| --- | --- |
| `package/`로 시작하는 브랜치 | 실행 화면의 Artifacts에 설치 파일 (확인용) |
| `make release` (`v<버전>` 태그) | GitHub 릴리스 생성. 위 다운로드 링크가 이 파일로 바뀜 |

`make release`는 `desktop/package.json`의 버전으로 태그를 만듭니다. 새 버전을 내려면 먼저 그 버전을 올리고 커밋하세요. 설치본은 개발 실행과 같은 `~/.paddock/extensions`·`~/.paddock/config`를 씁니다.

## 사용

- **작업 시작:** 앱을 켜면 첫 터미널이 왼쪽 Work의 Unassigned에 표시됩니다. 터미널에서 `cd`로 레포 폴더에 들어가 `claude`나 `codex`를 실행하면, 그 폴더가 작업 목록에 생기고 같은 묶음의 터미널과 열린 파일이 함께 연결됩니다. 어느 탭에서 실행해도 기존 탭과 분할 배치는 유지됩니다. 폴더를 고르는 창이나 등록 버튼은 없습니다. 한 번 들어온 터미널은 에이전트가 끝나도 남습니다.
- **같은 폴더에 터미널 더 열기:** 본문 바로 위 작업 폴더 탭 줄에 선택한 폴더의 터미널이 탭으로 나옵니다. 그 줄의 `＋`는 그 폴더에서 새 작업 터미널을 엽니다. 사이드바 폴더 줄의 `＋`도 같습니다. 기존 터미널의 셸과 Windows·WSL 실행 환경을 이어받습니다.
- **여러 레포 오가기:** 사이드바 **Work**에 작업 폴더와 그 아래 작업 터미널이 트리로 보입니다. 터미널을 누르면 전환되고, 두 번 누르면 이름을 바꿉니다. 오른쪽 흐린 글자는 실행 중인 프로그램입니다. 아래 구획에는 선택한 폴더의 파일이 나옵니다. 터미널 탭 줄 왼쪽의 사이드바 버튼이나 `Ctrl+B`(macOS `Cmd+B`)로 왼쪽 사이드바 전체를 접고 다시 펼칩니다.
- **정리:** 각 작업 터미널 탭에 항상 보이는 `×`를 누르면 종료 확인이 열립니다. 취소하면 유지하고 확인하면 해당 터미널만 닫으며 다른 터미널의 입력·출력은 유지됩니다. 폴더 줄의 `⋯` → **Remove from list**는 그 폴더의 터미널을 닫고 목록에서 뺍니다. 디스크의 폴더는 지우지 않습니다.
- **폴더 밖 터미널:** 왼쪽 Work 옆 `＋`는 기본 환경의 새 터미널 묶음을 Unassigned에 엽니다. 옆 `⌄`에서 실행 환경을 고릅니다(macOS zsh·bash, Windows PowerShell·명령 프롬프트·Git Bash·WSL). 아래 탭 줄의 `＋`로 내부 터미널을 추가하면 원래 터미널도 아래 탭에 함께 표시되어 클릭이나 `Ctrl+←/→`로 오갈 수 있습니다.
- **나누기:** `` Ctrl+Shift+` ``는 같은 폴더의 새 작업 터미널(추가 터미널을 보고 있으면 추가 터미널)을 아래 칸에, `Ctrl+Shift+5`(macOS `⌘\`)는 옆 칸에 엽니다. 각 칸 위 경로 줄의 버튼으로도 됩니다.
- **탭 이동:** 터미널에서 `Ctrl+←/→`는 같은 줄의 이전·다음 탭, `Ctrl+↑/↓`는 처음·마지막 탭으로 갑니다. `Ctrl+Shift+C`는 옆 칸에 새 터미널을 엽니다. 이 키들은 셸의 단어 이동·리눅스 터미널 복사 대신 쓰입니다.
- **빠른 설정:** 사이드바 위 보기 줄 오른쪽 끝 톱니를 누르면 색 테마·글자 크기·터미널 글꼴을 바로 바꾸고, All settings로 전체 설정을 엽니다.
- **탭이 많을 때:** 탭 줄 양끝의 `‹` `›`로 넘기고, 지금 보는 탭은 항상 보이게 따라갑니다. 칸을 나누면 입력이 가지 않는 칸은 흐려지고, 다른 칸에 떠 있는 탭은 점선 밑줄로 보입니다.
- **명령 팔레트:** `Ctrl+Shift+P`(또는 `F1`)로 모든 명령을 찾습니다. 예: **Reload Window**, **Preferences: Color Theme**(색 테마 바꾸기).
- **색·글꼴:** 기본 색 테마는 Paddock Dark(Paddock Light·Mermaid Dark·Mermaid Light도 있음)이고, 고른 테마가 창 틀(탭 줄·사이드바·상태 줄·팝업)까지 함께 바꿉니다. 터미널 글꼴은 Consolas(없으면 Menlo 등) 14px·자간 0·줄 높이 1.1, 한글은 맑은 고딕입니다. WSL에서 실행하면 Windows 글꼴 폴더를 함께 읽어 Windows 앱과 같은 글꼴로 보입니다(첫 실행만 약 2초 더 걸림). Windows 기본 셸은 WSL이고, WSL 배포판이 없으면 Git Bash, 그것도 없으면 PowerShell로 엽니다. 기본 셸은 설정(톱니바퀴)의 `Default shell`에서 바꿉니다. 셸 메뉴는 각 셸이 Windows에서 도는지 WSL의 Linux에서 도는지 함께 보여 줍니다(예: `Git Bash · Windows`, `WSL · Ubuntu`). Windows 앱에서도 터미널의 현재 폴더와 실행 중인 에이전트를 읽어 작업 폴더를 만듭니다. WSL 터미널은 WSL 안에서 직접 읽고, Git Bash·PowerShell·명령 프롬프트는 셸이 폴더를 바꿀 때 알리게 합니다(명령 프롬프트는 다음 프롬프트에서 알림). WSL 폴더에서 여는 터미널은 따로 고르지 않으면 WSL 셸입니다.
- **복사:** 터미널 글자를 드래그해 선택하면 바로 복사됩니다.
- **에이전트 완료 알림:** 보고 있지 않은 터미널에서 Claude·Codex 같은 에이전트가 5초 넘게 일하다 멈추면(끝났거나 답을 기다림) 짧은 소리가 나고, 사이드바·탭에 초록 점이 붙습니다. 해당 터미널을 현재 창에서 선택해 확인하면 점이 사라집니다. 앱 창만 활성화하거나 다른 터미널을 선택해도 미확인은 유지됩니다. 출력이 재개돼도 점은 남으며, 접힌 폴더와 Unassigned 묶음에도 점이 모여 표시됩니다. 점은 확인하지 않은 활동을 뜻하며 작업 완료를 확정하지 않습니다. 소리는 설정(Preferences)의 `paddock.agentDoneSound`로 끕니다. 사이드바 오른쪽 흐린 글자는 그 터미널에서 지금 실행 중인 프로그램입니다(Windows는 셸 이름).
- **에이전트 활동:** Work의 Claude·Codex 터미널에 `Waiting`(입력 뒤 첫 출력 대기)·`Working`(최근 출력)·`Quiet`(최근 출력 없음)을 점과 함께 표시합니다. 터미널 입력·출력으로 추정하므로 내부 계산이나 완료 여부를 확정하지 않으며, 좁은 사이드바에서는 이름과 프로그램·상태를 두 줄로 나눕니다.
- **Source control·Extensions:** 사이드바 위 아이콘으로 전환합니다. Source control은 작업 폴더의 변경·커밋을, Extensions는 VS Code처럼 Open VSX에서 확장을 찾아 설치합니다. `.vsix` 파일은 확장 보기로 끌어다 놓습니다.
- **원격:** 상태 줄 맨 왼쪽 원격 표시를 눌러 SSH 호스트에 연결합니다. 연결되면 창 전체가 그 호스트에서 동작합니다. 오른쪽 **Memory**는 시스템 메모리 사용률입니다.
- **여러 계정:** 본문 위 `Accounts`에서 Claude·Codex 실행 프로필을 이름으로 등록합니다. `Add & Open` 뒤 공식 CLI에서 로그인하고, 다른 프로필을 고르면 같은 폴더의 새 터미널을 엽니다. 기존 터미널은 유지합니다. 같은 실행 환경의 Claude 계정끼리, Codex 계정끼리 기존 스킬을 자동으로 공유하며 인증·설정 전체를 복사하지 않습니다. 이름 변경은 같은 프로필을 유지하고 Remove는 실행 목록에서만 제거합니다. WSL 계정의 로그인 웹 페이지는 Windows 기본 브라우저에서 열립니다. 변경 전 터미널에는 Accounts에서 새 터미널을 열어 적용합니다. 프로필별 로컬 사용량은 사용자가 붙인 이름과 함께 하단과 전체 계정 목록에서 확인합니다. Paddock은 프로필 메타데이터만 관리하고 로그인·인증 저장은 각 CLI가 담당합니다.
- **AI 사용량:** 상태 줄 오른쪽에 Claude·Codex별 마지막으로 선택한 계정 이름과 사용 한도가 나옵니다. 눌러 전체 계정을 비교하고 Open in new terminal로 다른 계정의 새 터미널을 열 수 있습니다. 긴 이름은 줄여 표시하며 좁은 창에서는 Usage 버튼으로 접힙니다. 전체 목록은 스크롤할 수 있고 계정이 많으면 이름으로 검색합니다. 기록이 없으면 No data, 조회에 실패하면 Unavailable로 표시하며 다른 계정의 값으로 대체하지 않습니다. 출처마다 표지(✱ Claude · ◎ Codex, Memory는 이름만)와 이름을 한 번 쓰고, 그 뒤에 `5h`(5시간 창)·`week`(주간 창) 게이지와 %가 붙습니다. 초기화된 창은 숨기지 않고 0%로 보입니다. 막대에 마우스를 올리면 남은 양과 초기화까지 남은 시간이 보입니다. 로그인은 필요 없습니다 — Codex는 세션 기록을, Claude는 Claude Code 상태 줄 입력(Pro·Max, 첫 응답 뒤)을 읽습니다. Claude 표시를 위해 선택한 계정의 `settings.json`에 Paddock 상태 줄을 넣습니다(원래 상태 줄은 이어서 보여 주고, 원본은 `settings.json.paddock-backup`). Claude 표시를 끄면 등록된 Claude 프로필과 기본 설정의 Paddock 상태 줄을 원래대로 되돌립니다. Claude·Codex 묶음은 사용량 목록을 열며 표시 여부는 Quick settings에서 바꿉니다. Memory는 눌러 숨기고 흐리게 남은 이름을 눌러 다시 켭니다. 설정(톱니바퀴)의 `Status bar`나 전체 설정의 `paddock.statusBar.claude`·`codex`·`memory`에서도 켜고 끕니다. 상태 줄 실행에 Node.js가 필요합니다(macOS·Linux는 없으면 앱 내장 런타임을 씁니다). 다른 Paddock(다른 설정 폴더의 설치본이나 시험 실행)이 이미 상태 줄을 쓰고 있으면 자동으로 바꾸지 않습니다. Default는 기존 기본 CLI 설정을 사용하며 Windows와 기본 WSL 기록 중 최근 값을 읽습니다. 등록 계정은 지정한 실행 환경과 WSL 배포판의 설정 위치에서만 읽습니다.
- **저장:** 하단 **Save** 또는 `⌘S`(Windows는 `Ctrl+S`)로 저장합니다. 저장하지 않고 닫으면 편집기의 저장 확인이 나타납니다.

기본 터미널 사용은 로컬에서 실행됩니다. 앱 자체가 외부로 보내는 것은 확장 검색어(Open VSX)뿐입니다. AI 사용량은 로컬 파일만 읽습니다. 사용자가 실행한 명령과 설치한 확장은 외부 서비스에 연결하거나 로컬 파일에 접근할 수 있습니다.

## 현재 범위

버전 0.1.0입니다. 로컬 터미널, 텍스트 편집·저장, VSIX 설치와 전용 편집기 연결을 검증했습니다. pen.dev는 자체 로그인을 요구하며 로그인 이후 도형 편집·저장은 미검증입니다. 모든 VS Code 확장이 호환되는 것은 아닙니다.

개발자 인증서로 서명한 설치 파일, 자동 업데이트, 확장 제거·업데이트 화면은 아직 제공하지 않습니다. 무료 오픈소스 공개를 목표로 하며 공개 배포 전 제품 라이선스를 확정해야 합니다.

## 개발

제품 코드는 `desktop/`에 있습니다. 저장소에서는 `.pen` 디자인 파일을 만들지 않습니다.


