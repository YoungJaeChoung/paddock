#!/usr/bin/env bash
# macOS 설치 파일(.dmg)을 만든다. bash 3.2(macOS 기본)에서 돈다.
#
#   native/target/mac/Paddock Native.app(build-mac-app.sh가 만든 묶음) → 임시 폴더에 앱 + /Applications 바로가기
#   → hdiutil로 압축 디스크 이미지 → native/target/mac/PaddockNative-<버전>-x64.dmg
#
# 사용법은 맥의 일반 설치 파일과 같다: dmg를 열고 Paddock Native를 Applications로 끌어 놓는다.
# 앱이 임시 서명이라 이 컴퓨터에서 만든 dmg는 그대로 열리지만, 다른 컴퓨터로 내려받은 dmg의 앱은 Gatekeeper가 막는다
# (그때는 Finder에서 앱을 우클릭 → 열기로 한 번 허용해야 한다).
# 묶음이 없으면 먼저 만들라고 알리고 끝낸다 — 빌드는 Makefile의 native-mac 몫이다.
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
mac_dir="$native_dir/target/mac"
bundle="$mac_dir/Paddock Native.app"
volume_name="Paddock Native"

[ -d "$bundle" ] || { echo "build-mac-dmg: $bundle is missing — run 'make native-mac' first" >&2; exit 1; }

# 버전은 native/Cargo.toml의 [workspace.package] version 하나에서 읽는다(build-mac-app.sh와 같은 awk 한 줄).
version="$(awk '/^\[workspace.package\]/{f=1;next} /^\[/{f=0} f&&/^version/{gsub(/"/,"",$3);print $3;exit}' "$native_dir/Cargo.toml")"
[ -n "$version" ] || { echo "build-mac-dmg: the workspace version was not found in Cargo.toml" >&2; exit 1; }
arch="$(uname -m)"
dmg="$mac_dir/PaddockNative-$version-$arch.dmg"

# 디스크 이미지에 담을 내용: 앱과, 끌어 놓을 자리인 /Applications 바로가기. 이 스크립트가 만든 임시 폴더만 지운다.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
ditto "$bundle" "$staging/Paddock Native.app"
ln -s /Applications "$staging/Applications"

# 서명이 묶음 복사 뒤에도 온전한지 확인한다. 깨졌으면 설치한 앱이 실행 즉시 끝난다.
codesign --verify --deep --strict "$staging/Paddock Native.app"

rm -f "$dmg"
hdiutil create -quiet -volname "$volume_name" -srcfolder "$staging" -fs HFS+ -format UDZO -ov "$dmg"
hdiutil verify -quiet "$dmg"
du -h "$dmg"
echo "build-mac-dmg: $dmg"
