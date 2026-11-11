#!/usr/bin/env bash
# macOS에서 release 실행 파일 세 개(paddock-app·paddock-daemon·paddock-pen-mcp)를 만들어
# native/target/mac/Paddock Native.app 묶음으로 담고 임시 서명(ad-hoc)한다. bash 3.2(macOS 기본)에서 돈다.
#
#   cargo build --release → Contents/MacOS/에 세 실행 파일
#   Windows 아이콘(.ico) → sips로 PNG → iconutil로 Contents/Resources/paddock-native.icns
#   Info.plist 작성 → codesign --force --deep -s - → 서명 검사
#
# 세 실행 파일은 Windows 설치 폴더와 같이 한 폴더(Contents/MacOS)에 나란히 둔다. 앱은 자기 실행 파일 옆에서
# 데몬(paddock-daemon)과 pen-mcp를 찾는다(pen-mcp는 macOS에서 pen_open만 동작하고 pen_save·pen_close는 거부한다).
# 앱은 gui 기본 기능으로만 빌드한다(자동 점검 기능 없음).
# Windows 빌드의 셰이더 사전 컴파일(fxc)은 macOS가 Metal 셰이더를 쓰므로 해당 없다.
#
# 서명: 개발자 인증서가 없어 임시 서명만 한다. 서명하지 않은 실행 파일은 이 macOS에서 실행 즉시 종료된다.
# 임시 서명한 앱은 이 컴퓨터에서 만든 것을 그대로 쓸 때만 문제없이 열린다(다른 컴퓨터로 내려받으면 Gatekeeper가 막는다).
# 생성물(.icns 포함)은 native/target/ 아래에만 생기며 git에 넣지 않는다.
set -euo pipefail

native_dir="$(cd "$(dirname "$0")/.." && pwd)"
release_dir="$native_dir/target/release"
mac_dir="$native_dir/target/mac"
bundle="$mac_dir/Paddock Native.app"
icon_source="$native_dir/crates/paddock-app/resources/paddock-native.ico"
bundle_id="dev.paddock.native"
min_macos="12.0"

# 버전은 native/Cargo.toml의 [workspace.package] version 하나에서 읽는다(Makefile·CI와 같은 awk 한 줄).
version="$(awk '/^\[workspace.package\]/{f=1;next} /^\[/{f=0} f&&/^version/{gsub(/"/,"",$3);print $3;exit}' "$native_dir/Cargo.toml")"
[ -n "$version" ] || { echo "build-mac-app: the workspace version was not found in Cargo.toml" >&2; exit 1; }

# 아이콘: .ico 안의 가장 큰 그림을 PNG로 뽑고(sips가 가장 큰 항목을 읽는다), 크기별로 줄여 iconset을 거쳐 .icns로 만든다.
make_icns() {
    local icns="$1"
    local work
    work="$(mktemp -d)"
    sips -s format png "$icon_source" --out "$work/base.png" >/dev/null
    local iconset="$work/paddock-native.iconset"
    mkdir "$iconset"
    local pixels
    # 원본이 256px이라 그보다 큰 크기(256@2x, 512)는 만들지 않는다. 확대본은 흐려질 뿐이다.
    for pixels in 16 32 64 128 256; do
        sips -z "$pixels" "$pixels" "$work/base.png" --out "$work/$pixels.png" >/dev/null
    done
    cp "$work/16.png" "$iconset/icon_16x16.png"
    cp "$work/32.png" "$iconset/icon_16x16@2x.png"
    cp "$work/32.png" "$iconset/icon_32x32.png"
    cp "$work/64.png" "$iconset/icon_32x32@2x.png"
    cp "$work/128.png" "$iconset/icon_128x128.png"
    cp "$work/256.png" "$iconset/icon_128x128@2x.png"
    cp "$work/256.png" "$iconset/icon_256x256.png"
    iconutil -c icns "$iconset" -o "$icns"
    rm -rf "$work"
}

write_info_plist() {
    cat >"$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>paddock-app</string>
    <key>CFBundleIconFile</key><string>paddock-native</string>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Paddock Native</string>
    <key>CFBundleDisplayName</key><string>Paddock Native</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>LSMinimumSystemVersion</key><string>$min_macos</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
    plutil -lint "$bundle/Contents/Info.plist" >/dev/null
}

# rustup이 설치하는 위치. 셸 설정을 읽지 않는 실행(IDE·cron)에서도 cargo를 찾게 한다.
PATH="$HOME/.cargo/bin:$PATH"
cd "$native_dir"
# 실행 파일이 요구하는 최소 macOS 버전을 Info.plist의 LSMinimumSystemVersion과 같게 맞춘다(안 주면 10.12로 찍힌다).
# 이 값은 cargo의 재빌드 판정에 들어가지 않아, 값을 바꾼 뒤에는 `cargo clean --release`로 다시 빌드해야 이미 만든 결과물에도 반영된다.
MACOSX_DEPLOYMENT_TARGET="$min_macos" cargo build --release -p paddock-app -p paddock-daemon -p paddock-pen-mcp

# 묶음은 매번 새로 만든다(남은 파일이 서명 봉인에 섞이지 않게).
rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
for exe in paddock-app paddock-daemon paddock-pen-mcp; do
    cp "$release_dir/$exe" "$bundle/Contents/MacOS/$exe"
done
make_icns "$bundle/Contents/Resources/paddock-native.icns"
write_info_plist

codesign --force --deep -s - "$bundle"
codesign --verify --deep --strict "$bundle"

echo "build-mac-app: $bundle (version $version)"
du -sh "$bundle"
ls -l "$bundle/Contents/MacOS"
