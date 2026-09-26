.PHONY: package-mac package-win release

# 설치 파일은 desktop/dist/에 생긴다. 각 운영체제에서 실행한다(맥 → .dmg, Windows → .exe).
# setup은 의존성 설치와 네이티브 모듈의 Electron용 컴파일을 맡는다. 이미 끝났으면 빠르게 지나간다.
package-mac:
	npm run setup
	npm run package:mac

package-win:
	npm run setup
	npm run package:win

# desktop/package.json의 버전으로 v<버전> 태그를 올린다. GitHub Actions가 맥(Apple Silicon·Intel)·Windows 설치 파일을 만들어 릴리스에 올린다.
# 새 버전을 내려면 먼저 desktop/package.json의 version을 올리고 커밋한다.
release:
	git tag v$$(node -p "require('./desktop/package.json').version")
	git push origin v$$(node -p "require('./desktop/package.json').version")
