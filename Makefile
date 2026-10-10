.PHONY: package-mac package-win release

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
