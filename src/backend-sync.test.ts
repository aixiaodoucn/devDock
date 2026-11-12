import { describe, expect, it } from 'vitest';
import { syncBackendModules } from './backend-mapping';
import type { BackendRepository, ScmModule } from './types';

const repo: BackendRepository = {
  id: 'local-api',
  name: '本地服务',
  path: '/example/api',
  remote: 'origin',
  scmGitId: '',
  scmModuleName: '',
  developmentBranch: 'old-development',
  testBranch: 'saved-testing',
};
const development: ScmModule = {
  gitId: '7',
  moduleName: 'example-api',
  gitRemoteUrl: 'https://example.invalid/team/api.git',
  branch: 'feature/current',
};

describe('SCM 自动填入与仓库范围', () => {
  it('只显示开发 SCM 匹配仓库，但保留所有已配置目录', () => {
    const unrelated = { ...repo, id: 'unrelated', name: '其他服务', path: '/example/unrelated' };
    const result = syncBackendModules(
      [repo, unrelated],
      {
        'local-api': 'git@example.invalid:team/api.git',
        unrelated: 'git@example.invalid:team/other.git',
      },
      [development],
      [{ ...development, branch: 'testing/current' }],
    );
    expect(result.visibleRepositoryIds).toEqual(['local-api']);
    expect(result.repositories).toHaveLength(2);
    expect(result.repositories[0]).toMatchObject({
      scmGitId: '7',
      scmModuleName: 'example-api',
      developmentBranch: 'feature/current',
      testBranch: 'testing/current',
    });
    expect(result.repositories[1]).toEqual(unrelated);
    expect(repo.developmentBranch).toBe('old-development');
    expect(result.warnings).toEqual([]);
  });
  it('只加载开发时保留原有测试分支，明确加载的测试缺失时清除旧目标并提示', () => {
    const local = { ...repo, scmGitId: '7' };
    expect(syncBackendModules([local], {}, [development]).repositories[0].testBranch).toBe(
      'saved-testing',
    );
    const result = syncBackendModules([local], {}, [development], []);
    expect(result.visibleRepositoryIds).toEqual(['local-api']);
    expect(result.repositories[0].testBranch).toBe('');
    expect(result.warnings.join(' ')).toContain('测试');
  });
  it('缺少本地匹配的 SCM 模块有提示，不按目录名称猜测', () => {
    const result = syncBackendModules([{ ...repo, name: development.moduleName }], {}, [
      development,
    ]);
    expect(result.visibleRepositoryIds).toEqual([]);
    expect(result.repositories[0]).toEqual({ ...repo, name: development.moduleName });
    expect(result.warnings.join(' ')).toContain('未匹配本地仓库');
  });
  it('开发歧义不展示或覆盖，测试歧义仅清除对应测试目标', () => {
    const local = { ...repo, scmGitId: '7' };
    const ambiguous = syncBackendModules([local], {}, [
      development,
      { ...development, branch: 'another-development' },
    ]);
    expect(ambiguous.visibleRepositoryIds).toEqual([]);
    expect(ambiguous.repositories[0]).toEqual(local);
    expect(ambiguous.warnings.join(' ')).toContain('唯一');
    const missingTest = syncBackendModules(
      [local],
      {},
      [development],
      [
        { ...development, branch: 'test-a' },
        { ...development, branch: 'test-b' },
      ],
    );
    expect(missingTest.visibleRepositoryIds).toEqual(['local-api']);
    expect(missingTest.repositories[0].testBranch).toBe('');
  });
  it('明确 Git ID 优先且仅按同一 ID 配对测试分支', () => {
    const result = syncBackendModules(
      [{ ...repo, scmGitId: '7' }],
      { 'local-api': 'https://different.example.invalid/repo.git' },
      [development],
      [{ ...development, gitId: '8', branch: 'wrong-target' }],
    );
    expect(result.visibleRepositoryIds).toEqual(['local-api']);
    expect(result.repositories[0].developmentBranch).toBe('feature/current');
    expect(result.repositories[0].testBranch).toBe('');
  });
  it('缺失开发仓库编号或分支时不自动填入不完整映射', () => {
    for (const source of [
      { ...development, gitId: null },
      { ...development, branch: '' },
    ]) {
      const result = syncBackendModules([repo], { 'local-api': development.gitRemoteUrl! }, [
        source,
      ]);
      expect(result.visibleRepositoryIds).toEqual([]);
      expect(result.repositories[0]).toEqual(repo);
      expect(result.warnings.length).toBeGreaterThan(0);
    }
  });
});
