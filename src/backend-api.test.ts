// @vitest-environment happy-dom
import { reactive, isProxy } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { api } from './api';
import type { BackendRepository } from './types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));
const repository: BackendRepository = {
  id: 'repo',
  name: '示例仓库',
  path: '/example/repo',
  remote: 'origin',
  scmGitId: '',
  scmModuleName: '',
  developmentBranch: 'feature/demo',
  testBranch: 'testing',
};

describe('后端 Git 与 SCM 桌面接口', () => {
  beforeEach(() => vi.resetAllMocks());
  it('扫描使用显式父目录，预检发送仓库快照且默认不推送', async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    await api.scanBackendChildren('/example/backend');
    expect(invoke).toHaveBeenLastCalledWith('scan_backend_children', {
      parentPath: '/example/backend',
    });
    const repositories = reactive([{ ...repository }]);
    await api.prepareBackendGit('group', repositories, 'merge');
    const [, args] = vi.mocked(invoke).mock.calls.at(-1)!;
    const payload = args as {
      repositories: BackendRepository[];
      push: boolean;
      keepTestPom: boolean;
    };
    expect(payload.push).toBe(false);
    expect(payload.keepTestPom).toBe(false);
    expect(isProxy(payload.repositories)).toBe(false);
    expect(isProxy(payload.repositories[0])).toBe(false);
    repositories[0].testBranch = 'changed';
    expect(payload.repositories[0].testBranch).toBe('testing');
  });
  it('远端地址查询发送仓库快照并返回按仓库 ID 索引的 URL', async () => {
    const urls = { repo: 'git@example.invalid:team/service.git' };
    vi.mocked(invoke).mockResolvedValue(urls);
    const repositories = reactive([{ ...repository }]);
    await expect(api.backendRepositoryRemoteUrls(repositories)).resolves.toEqual(urls);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('backend_repository_remote_urls', {
      repositories: [repository],
    });
    const [, args] = vi.mocked(invoke).mock.calls[0];
    const payload = args as { repositories: BackendRepository[] };
    expect(isProxy(payload.repositories[0])).toBe(false);
    repositories[0].remote = 'upstream';
    expect(payload.repositories[0].remote).toBe('origin');
  });
  it('预检与执行分离，只有明确请求推送才传 true', async () => {
    vi.mocked(invoke).mockResolvedValue('task');
    await api.prepareBackendGit('group', [repository], 'merge', true);
    expect(invoke).toHaveBeenLastCalledWith(
      'prepare_backend_git',
      expect.objectContaining({ push: true }),
    );
    expect(invoke).not.toHaveBeenCalledWith('execute_backend_git', expect.anything());
    await api.executeBackendGit('opaque-plan');
    expect(invoke).toHaveBeenLastCalledWith('execute_backend_git', { planId: 'opaque-plan' });
  });
  it.each(['switch-test', 'switch-master'] as const)(
    '分支切换 %s 不改写开发映射且不直接执行',
    async (operation) => {
      vi.mocked(invoke).mockResolvedValue({});
      await api.prepareBackendGit('group', [repository], operation);
      expect(invoke).toHaveBeenCalledExactlyOnceWith('prepare_backend_git', {
        projectId: 'group',
        repositories: [repository],
        operation,
        push: false,
        keepTestPom: false,
      });
      expect(repository.developmentBranch).toBe('feature/demo');
    },
  );
  it('开发分支推送显式标识远端写入，预检不会直接执行', async () => {
    vi.mocked(invoke).mockResolvedValue({});
    await api.prepareBackendGit('group', [repository], 'push-dev', true, false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('prepare_backend_git', {
      projectId: 'group',
      repositories: [repository],
      operation: 'push-dev',
      push: true,
      keepTestPom: false,
    });
    expect(invoke).not.toHaveBeenCalledWith('execute_backend_git', expect.anything());
  });
  it('pom 测试版本策略仅在用户显式选择后传入预检', async () => {
    vi.mocked(invoke).mockResolvedValue({});
    await api.prepareBackendGit('group', [repository], 'merge', false, true);
    expect(invoke).toHaveBeenLastCalledWith(
      'prepare_backend_git',
      expect.objectContaining({ keepTestPom: true, push: false }),
    );
    expect(invoke).not.toHaveBeenCalledWith('execute_backend_git', expect.anything());
  });
  it('继续只发送原计划标识并返回新预检计划', async () => {
    const plan = { planId: 'remaining' };
    vi.mocked(invoke).mockResolvedValue(plan);
    await expect(api.prepareRemainingBackendGit('paused')).resolves.toEqual(plan);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('prepare_remaining_backend_git', {
      planId: 'paused',
    });
  });
  it('选择 POM 仅发送计划、仓库标识和分支侧', async () => {
    await api.resolveBackendPomConflict('plan', 'repo', 'development');
    expect(invoke).toHaveBeenCalledExactlyOnceWith('resolve_backend_pom_conflict', {
      planId: 'plan',
      repoId: 'repo',
      side: 'development',
    });
  });
  it('逐块应用 POM 只发送文件、行号和选中的分支', async () => {
    const choices = [{ path: 'api/pom.xml', line: 12, side: 'test' }] as const;
    await api.resolveBackendPomBlocks('plan', 'repo', [...choices]);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('resolve_backend_pom_blocks', {
      planId: 'plan',
      repoId: 'repo',
      choices,
    });
  });
  it('在 IDEA 中打开只发送计划与仓库标识，不传文件路径', async () => {
    await api.openBackendWorktreeInIdea('plan', 'repo');
    expect(invoke).toHaveBeenCalledExactlyOnceWith('open_backend_worktree_in_idea', {
      planId: 'plan',
      repoId: 'repo',
    });
  });
  it('在 WebStorm 中打开只发送计划与仓库标识，不传文件路径', async () => {
    await api.openBackendWorktreeInWebStorm('plan', 'repo');
    expect(invoke).toHaveBeenCalledExactlyOnceWith('open_backend_worktree_in_webstorm', {
      planId: 'plan',
      repoId: 'repo',
    });
  });
  it('中止和手工完成仅发送计划与子仓库标识', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    await api.abortBackendGit('plan', 'repo');
    expect(invoke).toHaveBeenLastCalledWith('abort_backend_git', {
      planId: 'plan',
      repoId: 'repo',
    });
    await api.completeBackendGit('plan', 'repo');
    expect(invoke).toHaveBeenLastCalledWith('complete_backend_git', {
      planId: 'plan',
      repoId: 'repo',
    });
    await api.backendGitOperations('group');
    expect(invoke).toHaveBeenLastCalledWith('backend_git_operations', { projectId: 'group' });
  });
  it('清除 Git 记录只发送当前项目标识，返回实际删除的计划标识', async () => {
    vi.mocked(invoke).mockResolvedValue(['finished-plan']);
    await expect(api.backendClearGitOperations('group')).resolves.toEqual(['finished-plan']);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('clear_backend_git_operations', {
      projectId: 'group',
    });
  });
  it('清除 Git 记录传入选中的计划标识，返回实际删除的计划标识', async () => {
    vi.mocked(invoke).mockResolvedValue(['finished-plan', 'paused-plan']);
    await expect(api.backendClearGitOperations('group', 'paused-plan')).resolves.toEqual([
      'finished-plan',
      'paused-plan',
    ]);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('clear_backend_git_operations', {
      projectId: 'group',
      selectedPlanId: 'paused-plan',
    });
  });
  it('登录凭据只传入登录专用接口，不保存项目配置', async () => {
    vi.mocked(invoke).mockResolvedValue({ connected: true });
    await api.scmLogin('https://example.invalid', 'example-user', 'synthetic-password', false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('scm_login', {
      baseUrl: 'https://example.invalid',
      username: 'example-user',
      password: 'synthetic-password',
      remember: false,
    });
    await api.scmProjectModules('123');
    expect(invoke).toHaveBeenLastCalledWith('scm_project_modules', { projectId: '123' });
  });
});
