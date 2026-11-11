.PHONY: package-mac package-win release native-dev native-win native-lint native-release-win native-installer native-mac native-mac-install

# 설치 파일은 desktop/dist/에 생긴다. 각 운영체제에서 실행한다(맥 → .dmg, Windows → .exe).
# setup은 의존성 설치와 네이티브 모듈의 Electron용 컴파일을 맡는다. 이미 끝났으면 빠르게 지나간다.
package-mac:
	npm run setup
	npm run package:mac

package-win:
	npm run setup
	npm run package:win

# 새 버전을 내보낸다: 패치 버전을 올려 커밋하고(version 파일만 담는다), main과 v<버전> 태그를 푸시한다.
# 태그를 올리면 GitHub Actions가 맥(Apple Silicon·Intel)·Windows 설치 파일을 만들어 새 릴리스에 올린다.
# 다른 자리를 올리려면 BUMP=minor 또는 BUMP=major를 준다(make release BUMP=minor).
BUMP ?= patch
release:
	test "$$(git branch --show-current)" = main
	cd desktop && npm version $(BUMP) --no-git-tag-version
	npm version $(BUMP) --no-git-tag-version
	version=$$(node -p "require('./desktop/package.json').version") && \
	git add package.json desktop/package.json desktop/package-lock.json && \
	git commit -m "v$$version" -- package.json desktop/package.json desktop/package-lock.json && \
	git push origin main && \
	git tag v$$version && \
	git push origin v$$version

# 네이티브(GPUI) 앱. native-dev는 Linux·macOS에서 빌드·테스트하고, native-win은 WSL에서
# Windows용 exe를 만든다(cargo-xwin, clang 18 필요). Windows 실행 전 셰이더 원본을
# C: 쪽 같은 경로로 복사한다 — 크로스 빌드는 셰이더를 미리 컴파일하지 못한다.
# 개발용 Windows 빌드에는 자동 점검 대본(automation 기능)과 데몬의 시험 훅(test-hooks 기능)을 켠다. release 빌드에는 넣지 않는다.
# 빌드 전에 상한(10GB)을 넘은 결과물 프로필 폴더를 비운다(native/scripts/trim-target.sh) — 설정을 바꿔 가며
# 빌드하면 cargo가 옛 결과물을 지우지 않아 수십 GB까지 쌓인다.
NATIVE_PATH := $(HOME)/.local/bin:/usr/lib/llvm-18/bin:$(PATH)

# Linux에는 GPUI가 요구하는 시스템 라이브러리(fontconfig 등)가 없을 수 있어 창 부분(gui 기능)을 빼고 빌드한다.
# 시험은 임시 HOME에서 돌리고, 끝난 뒤 그 HOME에 .paddock이 생겼으면 실패로 본다 — 임시 홈을 쥐지 않은 시험이
# 사용자 홈에 쓰는 일을 한곳에서 잡는다. cargo·rustup 위치는 원래 홈의 것을 그대로 쓴다.
native-dev:
	native/scripts/trim-target.sh
	cd native && cargo build --workspace --no-default-features --features paddock-daemon/test-hooks \
		&& test_home=$$(mktemp -d) \
		&& CARGO_HOME="$${CARGO_HOME:-$(HOME)/.cargo}" RUSTUP_HOME="$${RUSTUP_HOME:-$(HOME)/.rustup}" HOME="$$test_home" \
			cargo test --workspace --no-default-features --features paddock-daemon/test-hooks \
		&& if [ -e "$$test_home/.paddock" ]; then echo "임시 홈을 쥐지 않은 시험이 HOME에 썼다: $$(find $$test_home/.paddock | head -5)"; rm -rf "$$test_home"; exit 1; fi \
		&& rm -rf "$$test_home"

native-win:
	native/scripts/trim-target.sh
	cd native && PATH="$(NATIVE_PATH)" XWIN_ACCEPT_LICENSE=1 \
		cargo xwin build --profile dev-win --target x86_64-pc-windows-msvc --workspace \
		--features paddock-app/automation,paddock-daemon/test-hooks
	native/scripts/copy-hlsl.sh

# 배포용 Windows 실행 파일 세 개(앱·데몬·pen-mcp)를 WSL에서 release로 크로스 빌드한다. native-win과 달리 셰이더를 실행 중에
# 컴파일하지 않는다 — Windows SDK의 fxc.exe(WSL에서 실행)로 미리 컴파일해 넣어, Windows CI의 release 빌드와 같은 꼴이 된다
# (native/scripts/build-release-win.sh). 자동 점검·시험 훅 기능은 켜지 않는다. 결과는 native/target/x86_64-pc-windows-msvc/release/.
native-release-win:
	native/scripts/trim-target.sh
	PATH="$(NATIVE_PATH)" native/scripts/build-release-win.sh

# 맥용 앱 묶음. native-mac은 release 실행 파일 세 개(앱·데몬·pen-mcp)를 macOS에서 빌드해
# native/target/mac/Paddock Native.app에 담고 임시 서명한다(native/scripts/build-mac-app.sh). 자동 점검 기능은 켜지 않는다.
# native-mac-install은 그 묶음을 ~/Applications/Paddock Native.app에 설치한다(있으면 교체, native/scripts/install-mac-app.sh).
# 설치할 자리의 앱이 실행 중이면 빌드 전에 멈춘다 — 실행 중인 데몬은 끝내지 않고, 새 앱이 이어 쓴다.
# Finder·Dock으로 연 앱은 앱이 시작할 때 로그인 셸의 PATH를 읽어 쓴다(claude·git 등을 찾게 한다).
native-mac:
	native/scripts/trim-target.sh
	native/scripts/build-mac-app.sh

native-mac-install:
	native/scripts/install-mac-app.sh --check
	$(MAKE) native-mac
	native/scripts/install-mac-app.sh

# 네이티브 앱 설치 파일(NSIS). 결과는 native/target/installer/PaddockNative-Setup-<버전>-x64.exe에 생긴다.
# NATIVE_EXE_DIR을 주지 않으면 native-release-win으로 release 실행 파일을 먼저 만든다(WSL).
# 이미 만든 실행 파일이 있으면 그 폴더를 준다(CI: make native-installer NATIVE_EXE_DIR=exes) — 그때는 Rust 도구 모음이 필요 없다.
# makensis 3.09는 native/target/tools/nsis/에 사용자 공간으로 받는다(fetch-makensis.sh). 담기 전에 실행 파일을 검사한다
# (check-payload.sh) — 셰이더를 실행 중에 컴파일하는 dev-win 실행 파일은 여기서 떨어진다.
# 버전은 native/Cargo.toml의 [workspace.package] version 하나에서 읽는다(CI도 같은 awk 한 줄을 쓴다).
NATIVE_RELEASE_DIR := native/target/x86_64-pc-windows-msvc/release
NATIVE_EXE_DIR ?= $(NATIVE_RELEASE_DIR)
NSIS_ROOT := $(CURDIR)/native/target/tools/nsis/root
native-installer: $(if $(filter $(NATIVE_RELEASE_DIR),$(NATIVE_EXE_DIR)),native-release-win)
	native/installer/fetch-makensis.sh
	PATH="$(NATIVE_PATH)" native/installer/check-payload.sh "$(NATIVE_EXE_DIR)"
	version=$$(awk '/^\[workspace.package\]/{f=1;next} /^\[/{f=0} f&&/^version/{gsub(/"/,"",$$3);print $$3;exit}' native/Cargo.toml) && \
	mkdir -p native/target/installer && \
	NSISDIR="$(NSIS_ROOT)/usr/share/nsis" "$(NSIS_ROOT)/usr/bin/makensis" -V2 -WX -DVERSION=$$version \
		-DSOURCE_DIR="$(abspath $(NATIVE_EXE_DIR))" -DICON="$(CURDIR)/native/crates/paddock-app/resources/paddock-native.ico" \
		-DOUT_FILE="$(CURDIR)/native/target/installer/PaddockNative-Setup-$$version-x64.exe" native/installer/paddock-native.nsi && \
	ls -l "native/target/installer/PaddockNative-Setup-$$version-x64.exe"

# 검토자가 한 줄로 도는 정적 점검: 서식 + Linux clippy(창 부분 제외) + Windows 대상 clippy(창 부분·자동 점검 기능 포함, 전 작업 공간)
# + 자동 점검을 끈 배포 꼴 clippy(자동 점검 전용 코드가 제품 코드를 조용히 지우는 `cfg` 실수를 경고로 잡는다).
# 데몬의 시험 훅(`test-hooks`)은 개발 빌드·시험·앞의 clippy에서만 켜고, 배포 꼴 clippy에는 켜지 않는다 — 훅이 꺼진 데몬이 경고 없이 빌드되는지 본다.
native-lint:
	native/scripts/trim-target.sh
	cd native && cargo fmt --check \
		&& cargo clippy --workspace --no-default-features --features paddock-daemon/test-hooks --all-targets -- -D warnings \
		&& PATH="$(NATIVE_PATH)" XWIN_ACCEPT_LICENSE=1 cargo xwin clippy --target x86_64-pc-windows-msvc --workspace \
			--features paddock-app/automation,paddock-daemon/test-hooks --all-targets -- -D warnings \
		&& PATH="$(NATIVE_PATH)" XWIN_ACCEPT_LICENSE=1 cargo xwin clippy --target x86_64-pc-windows-msvc \
			-p paddock-app -p paddock-daemon -- -D warnings
