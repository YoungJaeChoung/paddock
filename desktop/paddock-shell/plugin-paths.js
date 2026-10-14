// 작업 공간을 열지 않은 Paddock에서도 확장이 작업 공간 저장소(workspaceState·storageUri)를 쓰게 하는 저장 경로.
// Theia 기본 구현은 작업 공간이 없으면 저장 경로를 주지 않아, 확장이 저장하려 할 때 "Cannot save data: no opened workspace"로 조용히 실패한다.
const { injectable, decorate } = require('@theia/core/shared/inversify');
const { PluginPathsServiceImpl } = require('@theia/plugin-ext/lib/main/node/paths/plugin-paths-service');

/** 작업 공간이 없을 때 저장 폴더 이름(해시)의 입력으로 쓰는 고정 값. Paddock의 작업 공간은 앱 하나뿐이라 하나로 충분하다. */
const WORKSPACE_ID = 'paddock';

/**
 * 확장의 작업 공간 저장 폴더 경로를 돌려준다. 작업 공간이 없으면 앱 하나에 대응하는 고정 폴더를 쓴다.
 *
 * 작업 공간이 실제로 열려 있으면(폴더를 인자로 앱을 띄운 경우) 기본 동작 그대로 그 작업 공간의 폴더를 쓴다.
 * 폴더는 `<설정 폴더>/workspace-storage/<md5>`이며 없으면 만든다.
 *
 * Examples
 * --------
 * | workspaceUri          | 저장 폴더 해시의 입력      |
 * |-----------------------|----------------------------|
 * | `undefined` · `''`    | `'paddock'`                |
 * | `file:///home/me/app` | `file:///home/me/app` (기본 동작) |
 */
class PaddockPluginPathsService extends PluginPathsServiceImpl {
    async getHostStoragePath(
        workspaceUri,
        rootUris,
    ) {
        return super.getHostStoragePath(workspaceUri || WORKSPACE_ID, rootUris);
    }
}
decorate(injectable(), PaddockPluginPathsService);

module.exports = { PaddockPluginPathsService, WORKSPACE_ID };
