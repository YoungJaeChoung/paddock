#!/usr/bin/env bash
# 설치 파일에 담을 실행 파일 폴더를 검사한다. 하나라도 어긋나면 이유를 모두 보이고 실패한다.
#
#   1. paddock-app.exe·paddock-daemon.exe·paddock-pen-mcp.exe가 모두 있고, 각각 x86-64 Windows 실행 파일(PE)이다.
#   2. paddock-app.exe에 D3DCompileFromFile이 없다. 있으면 셰이더를 실행 중에 컴파일하는 빌드(dev-win 등)다 —
#      그 실행 파일은 빌드한 PC의 절대 경로에서 셰이더 원본을 읽어 다른 PC에서는 창을 그리지 못한다.
#   3. 세 실행 파일이 VCRUNTIME140.dll을 불러오지 않는다. 불러오면 VC++ 재배포 패키지가 없는 PC에서 시작하지 못한다.
#   4. paddock-app.exe에 아이콘(ICON)과 버전 정보(VERSIONINFO) 리소스가 있다.
#
# 쓰는 법: check-payload.sh <폴더>. llvm-readobj가 PATH에 있어야 한다.
set -euo pipefail

folder="${1:?usage: check-payload.sh <folder with the three release exes>}"
command -v llvm-readobj >/dev/null || { echo "check-payload: llvm-readobj not found in PATH" >&2; exit 1; }

n_failures=0
fail() {
    echo "check-payload: $1" >&2
    n_failures=$((n_failures + 1))
}

for exe in paddock-app.exe paddock-daemon.exe paddock-pen-mcp.exe; do
    path="$folder/$exe"
    if [ ! -f "$path" ]; then
        fail "$exe is missing"
        continue
    fi
    # 출력을 변수로 받은 뒤 찾는다 — 파이프로 grep -q에 넘기면 grep이 먼저 끝날 때 llvm-readobj가 SIGPIPE로 실패해 오판한다.
    # 실행 파일이 아니면 llvm-readobj가 실패한다 — 그 실패는 아래 형식 검사가 보고한다.
    headers="$(llvm-readobj --file-headers "$path" 2>&1 || true)"
    imports="$(llvm-readobj --coff-imports "$path" 2>&1 || true)"
    grep -q '^Format: COFF-x86-64$' <<<"$headers" || fail "$exe is not an x86-64 Windows executable"
    grep -qi 'Name: VCRUNTIME140' <<<"$imports" && fail "$exe loads VCRUNTIME140.dll (C runtime not linked in)"
    echo "check-payload: $exe $(stat -c %s "$path") bytes"
done

app="$folder/paddock-app.exe"
if [ -f "$app" ]; then
    grep -qa D3DCompileFromFile "$app" && fail "paddock-app.exe compiles shaders at run time (dev-win build?)"
    resources="$(llvm-readobj --coff-resources "$app" 2>&1 || true)"
    grep -q '^  Type: ICON ' <<<"$resources" || fail "paddock-app.exe has no ICON resource"
    grep -q '^  Type: VERSIONINFO ' <<<"$resources" || fail "paddock-app.exe has no VERSIONINFO resource"
fi

[ "$n_failures" -eq 0 ] || { echo "check-payload: $n_failures problem(s) in $folder" >&2; exit 1; }
echo "check-payload: $folder is ready to package"
