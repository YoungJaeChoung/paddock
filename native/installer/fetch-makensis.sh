#!/usr/bin/env bash
# 설치 파일 컴파일러 makensis(NSIS 3.09)를 native/target/tools/nsis/root/에 사용자 공간으로 받는다. 이미 있으면 건너뛴다.
#
# Ubuntu 패키지 nsis·nsis-common 3.09-4ubuntu1을 apt-get download로 받아 dpkg -x로 풀기만 한다 — 시스템에 설치하지 않고 sudo도 쓰지 않는다.
# 패키지는 apt가 서명을 확인한 저장소 목록에서 받는다. 받은 .deb는 이 실행만 쓰는 빈 폴더에 두고 끝나면 지운다.
# 패키지 목록이 없는 CI 실행기는 이 스크립트 전에 apt-get update를 돌린다(워크플로가 한다).
# 풀어 둔 makensis를 부를 때는 NSISDIR=<root>/usr/share/nsis를 준다 — 패키지의 기본 위치(/usr/share/nsis)에 풀지 않았기 때문이다.
set -euo pipefail

NSIS_PACKAGE_VERSION=3.09-4ubuntu1
# makensis -VERSION의 출력. Ubuntu 빌드는 패키지 판 번호(-4)를 붙인다.
NSIS_VERSION_OUTPUT=v3.09-4

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
tools_dir="$native_dir/target/tools/nsis"
makensis="$tools_dir/root/usr/bin/makensis"

# 풀어 둔 makensis를 NSISDIR로 부른다. 버전만 물어도 기본 스텁을 읽으므로 NSISDIR이 필요하다.
run_makensis() {
    local root="$1"
    shift
    NSISDIR="$root/usr/share/nsis" "$root/usr/bin/makensis" "$@"
}

if [ -x "$makensis" ] && [ "$(run_makensis "$tools_dir/root" -VERSION)" = "$NSIS_VERSION_OUTPUT" ]; then
    exit 0
fi

mkdir -p "$tools_dir"
download_dir="$(mktemp -d "$tools_dir/download.XXXXXX")"
trap 'rm -rf "$download_dir"' EXIT

(cd "$download_dir" && apt-get download "nsis=$NSIS_PACKAGE_VERSION" "nsis-common=$NSIS_PACKAGE_VERSION")
for package in "$download_dir"/*.deb; do
    dpkg -x "$package" "$download_dir/root"
done
fetched_version="$(run_makensis "$download_dir/root" -VERSION)"
[ "$fetched_version" = "$NSIS_VERSION_OUTPUT" ] || { echo "fetch-makensis: got makensis $fetched_version, want $NSIS_VERSION_OUTPUT" >&2; exit 1; }

# 다 푼 뒤에 한 번에 옮긴다 — 중간에 멈춰도 반쯤 푼 폴더가 "이미 있음"으로 보이지 않는다.
rm -rf "$tools_dir/root"
mv "$download_dir/root" "$tools_dir/root"
echo "fetch-makensis: makensis $NSIS_VERSION_OUTPUT in $tools_dir/root"
