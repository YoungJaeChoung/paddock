.PHONY: package-mac package-win release native-dev native-win native-lint

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

# 네이티브(GPUI) 앱. native-dev는 Linux에서 빌드·테스트하고, native-win은 WSL에서
# Windows용 exe를 만든다(cargo-xwin, clang 18 필요). Windows 실행 전 셰이더 원본을
# C: 쪽 같은 경로로 복사한다 — 크로스 빌드는 셰이더를 미리 컴파일하지 못한다.
# 개발용 Windows 빌드에는 자동 점검 대본(automation 기능)을 켠다. release 빌드에는 넣지 않는다.
NATIVE_PATH := $(HOME)/.local/bin:/usr/lib/llvm-18/bin:$(PATH)

# Linux에는 GPUI가 요구하는 시스템 라이브러리(fontconfig 등)가 없을 수 있어 창 부분(gui 기능)을 빼고 빌드한다.
# 시험은 임시 HOME에서 돌리고, 끝난 뒤 그 HOME에 .paddock이 생겼으면 실패로 본다 — 임시 홈을 쥐지 않은 시험이
# 사용자 홈에 쓰는 일을 한곳에서 잡는다. cargo·rustup 위치는 원래 홈의 것을 그대로 쓴다.
native-dev:
	cd native && cargo build --workspace --no-default-features \
		&& test_home=$$(mktemp -d) \
		&& CARGO_HOME="$${CARGO_HOME:-$(HOME)/.cargo}" RUSTUP_HOME="$${RUSTUP_HOME:-$(HOME)/.rustup}" HOME="$$test_home" \
			cargo test --workspace --no-default-features \
		&& if [ -e "$$test_home/.paddock" ]; then echo "임시 홈을 쥐지 않은 시험이 HOME에 썼다: $$(find $$test_home/.paddock | head -5)"; rm -rf "$$test_home"; exit 1; fi \
		&& rm -rf "$$test_home"

native-win:
	cd native && PATH="$(NATIVE_PATH)" XWIN_ACCEPT_LICENSE=1 \
		cargo xwin build --profile dev-win --target x86_64-pc-windows-msvc --workspace \
		--features paddock-app/automation
	native/scripts/copy-hlsl.sh

# 검토자가 한 줄로 도는 정적 점검: 서식 + Linux clippy(창 부분 제외) + Windows 대상 clippy(창 부분·자동 점검 기능 포함, 전 작업 공간).
native-lint:
	cd native && cargo fmt --check \
		&& cargo clippy --workspace --no-default-features --all-targets -- -D warnings \
		&& PATH="$(NATIVE_PATH)" XWIN_ACCEPT_LICENSE=1 cargo xwin clippy --target x86_64-pc-windows-msvc --workspace \
			--features paddock-app/automation --all-targets -- -D warnings
