; Paddock Native(네이티브 Paddock) Windows 설치 파일.
;
; 사용자별 설치다. 관리자 권한을 쓰지 않고 HKCU와 사용자 폴더에만 쓴다.
; 같은 PC의 Electron Paddock(%LOCALAPPDATA%\Programs\Paddock)과 그 피드백 사본(%LOCALAPPDATA%\PaddockFeedback)과 함께 쓴다:
; 그 폴더·레지스트리 키·바로 가기·프로세스는 읽기만 하고 바꾸지 않는다.
;
; 컴파일 입력(makensis -D):
;   VERSION     Cargo 작업 공간 버전(예: 0.1.0). 숫자 세 자리여야 한다 — 설치 파일의 버전 정보가 네 자리 숫자만 받는다.
;   SOURCE_DIR  paddock-app.exe·paddock-daemon.exe·paddock-pen-mcp.exe가 든 폴더(셰이더를 미리 컴파일한 release 빌드)
;   ICON        설치·제거 프로그램 아이콘(.ico)
;   OUT_FILE    만들 설치 파일 경로
;
; 실행 중인 네이티브 앱·데몬은 "설치 폴더 안의 그 파일이 잠겨 있는가"로만 판정한다. 프로세스를 이름으로 찾거나 끝내지 않는다 —
; 다른 폴더의 같은 이름 사본(개발 빌드 등)이나 Electron 앱이 판정에 끼어들 수 없다.
;
; 종료 코드(무인 설치·제거에서 호출한 쪽이 읽는다):
;   0  성공
;   1  사용자가 취소함(NSIS 기본값)
;   2  스크립트가 중단함(NSIS 기본값)
;   3~6은 아래 EXIT_* 참고

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef VERSION | SOURCE_DIR | ICON | OUT_FILE
  !error "Pass -DVERSION, -DSOURCE_DIR, -DICON and -DOUT_FILE to makensis"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

; -- 이름과 위치 --
!define PRODUCT_NAME "Paddock Native"
!define PUBLISHER "Paddock"
!define APP_EXE "paddock-app.exe"
!define DAEMON_EXE "paddock-daemon.exe"
!define PEN_MCP_EXE "paddock-pen-mcp.exe"
!define UNINSTALLER_EXE "Uninstall Paddock Native.exe"
!define SHORTCUT_NAME "Paddock Native.lnk"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockNative"
; 실행 중이라 덮어쓸 수 없던 데몬·pen-mcp를 옮겨 두는 설치 폴더 안 폴더. 그 아래 번호 폴더(1, 2, …)에 원래 이름 그대로 둔다.
!define REPLACED_DIR "replaced"
; 제거할 때 실행 중이라 지울 수 없던 pen-mcp를 옮겨 두는 임시 폴더.
!define REMOVED_DIR "$TEMP\PaddockNativeRemoved"
; 앱이 쓰는 Windows 쪽 공용 Claude 설정의 Pen 연동 MCP 설정 파일.
!define PEN_MCP_CONFIG "$PROFILE\.paddock\config\claude\pen-mcp.json"
; Electron Paddock이 실제 설치 위치(InstallLocation)를 적어 두는 키. 읽기만 한다.
!define ELECTRON_APP_KEY "Software\9aa88f6f-06f5-5270-9789-4b07f3891133"

; -- 종료 코드 --
!define EXIT_APP_RUNNING 3
!define EXIT_FOLDER_NOT_ALLOWED 4
!define EXIT_DAEMON_RUNNING 5
!define EXIT_MOVE_FAILED 6

; -- 안내 글 --
!define MSG_APP_RUNNING "Paddock Native is running. Close it, then click Retry."
!define MSG_DAEMON_RUNNING "Paddock Native terminals are still running. In Paddock Native, choose $\"Quit and Stop All Terminals$\", then click Retry."
!define MSG_MOVE_FAILED "A running Paddock Native file couldn't be moved out of the way. In Paddock Native, choose $\"Quit and Stop All Terminals$\", then try again."

Name "${PRODUCT_NAME}"
OutFile "${OUT_FILE}"
; 폴더 이름은 Paddock으로 시작하면 안 된다 — 아래 FolderAllowed 참고.
InstallDir "$LOCALAPPDATA\Programs\NativePaddock"
; 업그레이드할 때 사용자가 앞서 고른 폴더를 다시 쓴다.
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
BrandingText "${PRODUCT_NAME} ${VERSION}"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"

!define MUI_DIRECTORYPAGE_TEXT_TOP "Setup will install ${PRODUCT_NAME} in the folder below. Folders of the Paddock desktop app can't be used."
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE DirectoryLeave
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
; Finish 화면의 실행 체크 상자는 앱을 설치 프로그램의 자식 프로세스로 띄운다.
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${PRODUCT_NAME}"
; 읽어보기(README) 체크 상자 자리를 바탕 화면 바로 가기 선택에 쓴다. 기본은 꺼짐이다.
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"


; ---------------------------------------------------------------------------
; 설치·제거 프로그램이 함께 쓰는 함수. NSIS는 제거 프로그램 쪽 함수에 un. 접두어를 요구해 매크로로 두 벌 만든다.
; ---------------------------------------------------------------------------

; 파일이 잠겨 있는지(실행 중인 실행 파일인지) 본다. 스택: 파일 경로 → "1"(잠김) 또는 "0".
; 쓰기용으로 열리지 않으면 잠긴 것으로 본다. 연 뒤 아무것도 쓰지 않고 닫으므로 내용·수정 시각은 바뀌지 않는다.
; 파일이 없으면 "0"이다 — 쓰기용 열기는 없는 파일을 만들기 때문에 먼저 확인한다.
!macro DEFINE_FILE_LOCKED prefix
Function ${prefix}FileLocked
  Exch $0
  Push $1
  ${If} ${FileExists} "$0"
    ClearErrors
    FileOpen $1 "$0" a
    ${If} ${Errors}
      StrCpy $0 1
    ${Else}
      FileClose $1
      StrCpy $0 0
    ${EndIf}
  ${Else}
    StrCpy $0 0
  ${EndIf}
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro DEFINE_FILE_LOCKED ""
!insertmacro DEFINE_FILE_LOCKED "un."

; 파일이 잠겨 있는 동안 진행하지 않는다. 스택(위부터): 파일 경로, 안내 글, 종료 코드. 남기는 값은 없다.
; 대화형: 안내 글과 Retry/Cancel을 보여 준다. Retry면 다시 보고, Cancel이면 그 종료 코드로 끝낸다.
; 무인(/S): 묻지 않고 바로 그 종료 코드로 끝낸다.
!macro DEFINE_REQUIRE_UNLOCKED prefix
Function ${prefix}RequireUnlocked
  Exch $R0
  Exch
  Exch $R1
  Exch 2
  Exch $R2
  Push $R3
  ${Do}
    Push $R0
    Call ${prefix}FileLocked
    Pop $R3
    ${If} $R3 != 1
      ${ExitDo}
    ${EndIf}
    ${If} ${Silent}
      SetErrorLevel $R2
      Quit
    ${EndIf}
    ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$R1" IDCANCEL`
      SetErrorLevel $R2
      Quit
    ${EndIf}
  ${Loop}
  Pop $R3
  Pop $R2
  Pop $R0
  Pop $R1
FunctionEnd
!macroend
!insertmacro DEFINE_REQUIRE_UNLOCKED ""
!insertmacro DEFINE_REQUIRE_UNLOCKED "un."

; 폴더 아래에서 아직 없는 번호 폴더 경로를 고른다. 스택: 부모 폴더 → "<부모>\<N>"(N은 1부터). 폴더는 만들지 않는다.
!macro DEFINE_FREE_NUMBERED_FOLDER prefix
Function ${prefix}FreeNumberedFolder
  Exch $0
  Push $1
  StrCpy $1 1
  ${While} ${FileExists} "$0\$1"
    IntOp $1 $1 + 1
  ${EndWhile}
  StrCpy $0 "$0\$1"
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro DEFINE_FREE_NUMBERED_FOLDER ""
!insertmacro DEFINE_FREE_NUMBERED_FOLDER "un."

; 실행 중인 파일을 옮기지 못했을 때: 대화형이면 안내한 뒤, EXIT_MOVE_FAILED로 끝낸다.
!macro DEFINE_MOVE_FAILED prefix
Function ${prefix}MoveFailed
  ${IfNot} ${Silent}
    MessageBox MB_OK|MB_ICONSTOP "${MSG_MOVE_FAILED}"
  ${EndIf}
  SetErrorLevel ${EXIT_MOVE_FAILED}
  Quit
FunctionEnd
!macroend
!insertmacro DEFINE_MOVE_FAILED ""
!insertmacro DEFINE_MOVE_FAILED "un."


; ---------------------------------------------------------------------------
; 설치
; ---------------------------------------------------------------------------

; 문자열이 앞부분으로 시작하는지 본다(대소문자 무시). 스택(위부터): 앞부분, 문자열 → "1" 또는 "0". 앞부분이 비면 "0"이다.
Function StartsWith
  Exch $0
  Exch
  Exch $1
  Push $2
  StrLen $2 $0
  StrCpy $2 $1 $2
  ${If} $0 != ""
  ${AndIf} $2 == $0
    StrCpy $0 1
  ${Else}
    StrCpy $0 0
  ${EndIf}
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; $INSTDIR에 설치해도 되는지 본다. 스택에 "1"(허용) 또는 "0"(거부)을 남긴다.
;
; 거부하는 폴더: Electron Paddock의 기본 위치 Programs\Paddock, Electron이 적어 둔 실제 설치 위치, 피드백 사본 PaddockFeedback.
; 이유: Electron의 설치·제거 프로그램은 경로가 자기 설치 폴더 문자열로 시작하는 프로세스를 모두 끝낸다(대소문자 무시, 끝의 \ 없이 비교).
; 피드백 사본을 다시 띄우는 단계도 PaddockFeedback 아래의 프로세스를 끝낸다. 그 아래에 두면 Electron을 업데이트할 때마다
; 네이티브 앱과 데몬(살아 있는 터미널)이 함께 끝난다.
; 같은 이유로 비교 대상은 폴더가 아니라 "<폴더>\paddock-app.exe"다 — 세 실행 파일이 모두 paddock-로 시작하므로,
; 폴더가 Programs 자체여도 실행 파일 경로가 Programs\Paddock…으로 시작해 함께 걸린다.
;
; Examples(LOCALAPPDATA 아래 기준)
;   Programs\NativePaddock          → 1
;   Programs\Paddock                → 0
;   Programs\PaddockGuardProbe      → 0
;   Programs                        → 0
;   PaddockFeedbackGuardProbe       → 0
;   D:\Tools\Paddock Native         → 1
Function FolderAllowed
  Push $0
  Push $1
  Push $2
  Push $3
  StrCpy $0 1
  StrCpy $2 "$INSTDIR\${APP_EXE}"

  Push $2
  Push "$LOCALAPPDATA\Programs\Paddock"
  Call StartsWith
  Pop $1
  ${If} $1 == 1
    StrCpy $0 0
  ${EndIf}

  Push $2
  Push "$LOCALAPPDATA\PaddockFeedback"
  Call StartsWith
  Pop $1
  ${If} $1 == 1
    StrCpy $0 0
  ${EndIf}

  ; Electron이 기본 위치가 아닌 곳에 설치됐을 때. 값 끝의 \는 떼고 비교한다.
  ReadRegStr $1 HKCU "${ELECTRON_APP_KEY}" "InstallLocation"
  ${If} $1 != ""
    StrCpy $3 $1 1 -1
    ${If} $3 == "\"
      StrCpy $1 $1 -1
    ${EndIf}
    Push $2
    Push $1
    Call StartsWith
    Pop $1
    ${If} $1 == 1
      StrCpy $0 0
    ${EndIf}
  ${EndIf}

  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; 대화형: 거부한 폴더면 다음 단추를 끈다.
Function .onVerifyInstDir
  Push $0
  Call FolderAllowed
  Pop $0
  ${If} $0 != 1
    Pop $0
    Abort
  ${EndIf}
  Pop $0
FunctionEnd

; 무인: 화면이 없으므로 폴더 검사와 실행 중 확인을 여기서 한다(/D=와 InstallDirRegKey가 이미 적용된 $INSTDIR을 본다).
; 걸리면 아무것도 쓰지 않고 끝낸다.
Function .onInit
  ${If} ${Silent}
    Call FolderAllowed
    Pop $0
    ${If} $0 != 1
      SetErrorLevel ${EXIT_FOLDER_NOT_ALLOWED}
      Quit
    ${EndIf}
    Push ${EXIT_APP_RUNNING}
    Push "${MSG_APP_RUNNING}"
    Push "$INSTDIR\${APP_EXE}"
    Call RequireUnlocked
  ${EndIf}
FunctionEnd

; 대화형: 폴더를 고른 뒤, 그 폴더의 앱이 돌고 있으면 닫을 때까지 기다린다.
Function DirectoryLeave
  Push ${EXIT_APP_RUNNING}
  Push "${MSG_APP_RUNNING}"
  Push "$INSTDIR\${APP_EXE}"
  Call RequireUnlocked
FunctionEnd

; 실행 중이라 덮어쓸 수 없는 데몬·pen-mcp를 $INSTDIR\replaced\<N>\으로 옮긴다. 이름은 바꾸지 않는다.
;
; 이름을 유지하는 이유: 새 앱은 지문이 다른 데몬을 "오래됨"으로 보고 교체할 때 실행 파일 이름(paddock-daemon.exe)으로
; 그 프로세스를 확인한다. .old처럼 이름을 바꾸면 그 교체가 옛 데몬을 알아보지 못한다.
; 옮겨진 프로세스는 계속 돈다: 데몬은 터미널을 쥔 채로, pen-mcp는 그것을 띄운 Claude 세션이 끝날 때까지.
; 하나라도 옮기지 못하면 이미 옮긴 파일을 되돌리고 EXIT_MOVE_FAILED로 끝낸다.
Function MoveLockedHelpers
  Push $0
  Push $1
  Push $2
  Push "$INSTDIR\${DAEMON_EXE}"
  Call FileLocked
  Pop $0
  Push "$INSTDIR\${PEN_MCP_EXE}"
  Call FileLocked
  Pop $1
  ${If} $0 == 1
  ${OrIf} $1 == 1
    Push "$INSTDIR\${REPLACED_DIR}"
    Call FreeNumberedFolder
    Pop $2
    CreateDirectory "$2"
    ${If} $0 == 1
      ClearErrors
      Rename "$INSTDIR\${DAEMON_EXE}" "$2\${DAEMON_EXE}"
      ${If} ${Errors}
        Call MoveFailed
      ${EndIf}
    ${EndIf}
    ${If} $1 == 1
      ClearErrors
      Rename "$INSTDIR\${PEN_MCP_EXE}" "$2\${PEN_MCP_EXE}"
      ${If} ${Errors}
        ${If} $0 == 1
          Rename "$2\${DAEMON_EXE}" "$INSTDIR\${DAEMON_EXE}"
        ${EndIf}
        Call MoveFailed
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${SHORTCUT_NAME}" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"

  ; 앞선 설치가 옮겨 둔 옛 파일을 지운다. 그 프로세스가 아직 돌면 그 파일은 남고, 다음 설치나 제거가 다시 지운다.
  RMDir /r "$INSTDIR\${REPLACED_DIR}"
  Call MoveLockedHelpers

  File "${SOURCE_DIR}\${APP_EXE}"
  File "${SOURCE_DIR}\${DAEMON_EXE}"
  File "${SOURCE_DIR}\${PEN_MCP_EXE}"
  WriteUninstaller "$INSTDIR\${UNINSTALLER_EXE}"

  CreateShortcut "$SMPROGRAMS\${SHORTCUT_NAME}" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  ; 이 키 말고 다른 레지스트리 키는 쓰지 않는다(앱 자체도 레지스트리를 쓰지 않는다).
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${APP_EXE}",0'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER_EXE}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER_EXE}" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ; 설정 앱의 앱 목록에 보이는 크기(KB).
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
SectionEnd


; ---------------------------------------------------------------------------
; 제거
; ---------------------------------------------------------------------------

; 문자열이 다른 문자열을 포함하는지 본다(대소문자 무시). 스택(위부터): 찾을 문자열, 문자열 → "1" 또는 "0".
Function un.Contains
  Exch $0
  Exch
  Exch $1
  Push $2
  Push $3
  Push $4
  StrLen $2 $0
  StrCpy $3 0
  StrCpy $4 0
  ${If} $2 > 0
    ${Do}
      StrCpy $4 $1 $2 $3
      ${If} $4 == $0
        StrCpy $4 1
        ${ExitDo}
      ${EndIf}
      ${If} $4 == ""
        StrCpy $4 0
        ${ExitDo}
      ${EndIf}
      IntOp $3 $3 + 1
    ${Loop}
  ${EndIf}
  StrCpy $0 $4
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; 경로를 JSON 문자열 안의 꼴로 바꾼다(\를 \\로). 스택: 경로 → 바꾼 경로.
;
; Examples
;   C:\Users\me\NativePaddock\paddock-pen-mcp.exe → C:\\Users\\me\\NativePaddock\\paddock-pen-mcp.exe
Function un.JsonEscapeBackslashes
  Exch $0
  Push $1
  Push $2
  Push $3
  StrCpy $1 ""
  StrCpy $3 0
  ${Do}
    StrCpy $2 $0 1 $3
    ${If} $2 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $2 == "\"
      StrCpy $1 "$1\\"
    ${Else}
      StrCpy $1 "$1$2"
    ${EndIf}
    IntOp $3 $3 + 1
  ${Loop}
  StrCpy $0 $1
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; Windows 쪽 공용 Claude 설정의 pen-mcp.json이 이 설치 폴더의 paddock-pen-mcp.exe를 가리킬 때만 지운다.
; 지우지 않으면 공용 claude 래퍼가 없어진 프로그램을 MCP 서버로 계속 넘긴다.
; 다른 사본(개발 빌드 등)을 가리키는 파일과 WSL 쪽 사본은 그대로 둔다 — 다음에 실행되는 네이티브 앱이 다시 쓰거나 지운다.
; 줄 단위로 읽어 찾는다. 경로에 시스템 코드 페이지 밖의 글자가 있으면 찾지 못하고 파일을 남긴다(지우는 쪽으로는 틀리지 않는다).
Function un.RemovePenMcpConfig
  Push $0
  Push $1
  Push $2
  Push $3
  ${If} ${FileExists} "${PEN_MCP_CONFIG}"
    Push "$INSTDIR\${PEN_MCP_EXE}"
    Call un.JsonEscapeBackslashes
    Pop $0
    StrCpy $3 0
    ClearErrors
    FileOpen $1 "${PEN_MCP_CONFIG}" r
    ${IfNot} ${Errors}
      ${Do}
        ClearErrors
        FileRead $1 $2
        ${If} ${Errors}
          ${ExitDo}
        ${EndIf}
        Push $2
        Push $0
        Call un.Contains
        Pop $3
        ${If} $3 == 1
          ${ExitDo}
        ${EndIf}
      ${Loop}
      FileClose $1
    ${EndIf}
    ${If} $3 == 1
      Delete "${PEN_MCP_CONFIG}"
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; 잠긴 pen-mcp 하나를 $TEMP\PaddockNativeRemoved\<N>\으로 옮긴다. 스택: 파일 경로. 남기는 값은 없다.
; Electron 안의 Claude 세션도 공용 래퍼를 거쳐 이 프로그램을 띄울 수 있다 — 제거하려고 그 세션까지 닫게 하지 않는다.
Function un.MoveLockedPenMcp
  Exch $0
  Push $1
  Push $0
  Call un.FileLocked
  Pop $1
  ${If} $1 == 1
    Push "${REMOVED_DIR}"
    Call un.FreeNumberedFolder
    Pop $1
    CreateDirectory "$1"
    ClearErrors
    Rename "$0" "$1\${PEN_MCP_EXE}"
    ${If} ${Errors}
      Call un.MoveFailed
    ${EndIf}
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

; 지우기 전에 확인한다. 하나라도 걸리면 아무것도 지우지 않는다.
; 데몬은 강제로 끝내지 않는다 — 살아 있는 터미널을 쥐고 있고, 앱을 지운 뒤에는 그 터미널을 다시 열 방법이 없다.
; replaced 아래에 옮겨 둔 옛 데몬도 같은 이유로 본다.
Function un.onInit
  Push $0
  Push $1
  Push ${EXIT_APP_RUNNING}
  Push "${MSG_APP_RUNNING}"
  Push "$INSTDIR\${APP_EXE}"
  Call un.RequireUnlocked

  Push ${EXIT_DAEMON_RUNNING}
  Push "${MSG_DAEMON_RUNNING}"
  Push "$INSTDIR\${DAEMON_EXE}"
  Call un.RequireUnlocked

  FindFirst $0 $1 "$INSTDIR\${REPLACED_DIR}\*"
  ${DoWhile} $1 != ""
    ${If} $1 != "."
    ${AndIf} $1 != ".."
      Push ${EXIT_DAEMON_RUNNING}
      Push "${MSG_DAEMON_RUNNING}"
      Push "$INSTDIR\${REPLACED_DIR}\$1\${DAEMON_EXE}"
      Call un.RequireUnlocked
    ${EndIf}
    FindNext $0 $1
  ${Loop}
  FindClose $0
  Pop $1
  Pop $0
FunctionEnd

; 이 설치 프로그램이 만든 이름만 지운다. ~/.paddock(설정·상태·터미널 기록·공용 설정)과 Electron 쪽 항목은 남긴다.
Section "Uninstall"
  ; 실행 중인 pen-mcp를 먼저 옮긴다. 옮기지 못하면 아무것도 지우지 않고 끝난다.
  Push "$INSTDIR\${PEN_MCP_EXE}"
  Call un.MoveLockedPenMcp
  FindFirst $0 $1 "$INSTDIR\${REPLACED_DIR}\*"
  ${DoWhile} $1 != ""
    ${If} $1 != "."
    ${AndIf} $1 != ".."
      Push "$INSTDIR\${REPLACED_DIR}\$1\${PEN_MCP_EXE}"
      Call un.MoveLockedPenMcp
    ${EndIf}
    FindNext $0 $1
  ${Loop}
  FindClose $0

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${DAEMON_EXE}"
  Delete "$INSTDIR\${PEN_MCP_EXE}"
  Delete "$INSTDIR\${UNINSTALLER_EXE}"
  RMDir /r "$INSTDIR\${REPLACED_DIR}"
  ; 비었을 때만 지운다 — 사용자가 같은 폴더에 둔 다른 파일은 남긴다.
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${SHORTCUT_NAME}"
  Delete "$DESKTOP\${SHORTCUT_NAME}"
  Call un.RemovePenMcpConfig
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
