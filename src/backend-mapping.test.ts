import { describe, expect, it } from 'vitest';
import { mapBackendModules, scmProjectId } from './backend-mapping';
import type { BackendRepository, ScmModule } from './types';
const repo: BackendRepository = {
  id: 'repo',
  name: 'same-name',
  path: '/example/same-name',
  remote: 'origin',
  scmGitId: '',
  scmModuleName: '',
  developmentBranch: '',
  testBranch: '',
};
const module: ScmModule = {
  gitId: '42',
  moduleName: 'same-name',
  gitRemoteUrl: 'https://example.invalid/team/service.git',
  branch: 'develop',
};
describe('SCM 映射', () => {
  it('仅根据唯一 URL 或 gitId 匹配，不猜目录名', () => {
    expect(() => mapBackendModules([repo], {}, [module], [])).toThrow('无法唯一匹配');
    const result = mapBackendModules(
      [repo],
      { repo: 'git@example.invalid:team/service.git' },
      [module],
      [{ ...module, branch: 'test' }],
    );
    expect(result[0]).toMatchObject({
      scmGitId: '42',
      developmentBranch: 'develop',
      testBranch: 'test',
    });
  });
  it('测试分支必须来自同 gitId，拒绝歧义与覆盖已有冲突映射', () => {
    expect(() =>
      mapBackendModules([{ ...repo, scmGitId: '42' }], {}, [module], [{ ...module, gitId: '99' }]),
    ).toThrow();
    expect(() =>
      mapBackendModules(
        [repo],
        { repo: module.gitRemoteUrl! },
        [module, { ...module, gitId: '43' }],
        [],
      ),
    ).toThrow();
  });
  it('自动匹配保留非默认端口，避免把不同 Git 服务当作同一仓库', () => {
    expect(() =>
      mapBackendModules(
        [repo],
        { repo: 'ssh://git@example.invalid:2222/team/service.git' },
        [module],
        [],
      ),
    ).toThrow('无法唯一匹配');
    expect(
      mapBackendModules([repo], { repo: 'ssh://git@example.invalid:22/team/service.git' }, [
        module,
      ])[0].scmGitId,
    ).toBe('42');
  });
  it('只应用开发映射时清空既有测试分支', () => {
    const result = mapBackendModules(
      [{ ...repo, testBranch: 'saved-testing' }],
      { repo: module.gitRemoteUrl! },
      [module],
    );
    expect(result[0]).toMatchObject({ developmentBranch: 'develop', testBranch: '' });
    expect(repo.testBranch).toBe('');
  });
  it('明确加载测试映射时空结果或不匹配不能清空旧目标', () => {
    expect(() => mapBackendModules([repo], { repo: module.gitRemoteUrl! }, [module], [])).toThrow(
      '测试分支',
    );
    const result = mapBackendModules(
      [{ ...repo, testBranch: 'saved-testing' }],
      { repo: module.gitRemoteUrl! },
      [module],
      [{ ...module, branch: 'selected-testing' }],
    );
    expect(result[0].testBranch).toBe('selected-testing');
  });
  it('仅解析数字或完整项目链接，不请求链接', () => {
    expect(scmProjectId('https://example.invalid/projects/123?tab=modules')).toBe('123');
    expect(scmProjectId(' 123 ')).toBe('123');
    expect(() => scmProjectId('https://example.invalid/project/no-id')).toThrow();
  });
});
