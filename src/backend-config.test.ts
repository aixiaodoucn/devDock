import { describe, expect, it } from 'vitest';
import { defaultConfig, newProject, normalizeProjectPlatforms, mergeConfig } from './types';

describe('后端项目配置兼容与元数据', () => {
  it('旧配置补空子项目与SCM关联，不改变原对象', () => {
    const raw = JSON.parse(JSON.stringify(newProject()));
    delete raw.subProjects;
    delete raw.scmDevelopmentProjectId;
    delete raw.scmTestProjectId;
    const normalized = normalizeProjectPlatforms(raw);
    expect(normalized.subProjects).toEqual([]);
    expect(normalized.scmDevelopmentProjectId).toBe('');
    expect(normalized.scmTestProjectId).toBe('');
    expect(raw).not.toHaveProperty('subProjects');
  });
  it('子项目仅保存约定元数据，不复制附带的登录凭据', () => {
    const project = {
      ...newProject(),
      type: 'backend' as const,
      platforms: [],
      subProjects: [
        {
          id: 'repo',
          name: '子项目',
          path: '/example/repo',
          remote: 'origin',
          scmGitId: '1',
          scmModuleName: 'example-repo',
          developmentBranch: 'feature/demo',
          testBranch: 'testing',
          password: 'synthetic-password',
          cookie: 'synthetic-cookie',
        },
      ],
    };
    const normalized = normalizeProjectPlatforms(project);
    expect(normalized.subProjects[0]).not.toHaveProperty('password');
    expect(normalized.subProjects[0]).not.toHaveProperty('cookie');
    expect(normalized.subProjects[0].developmentBranch).toBe('feature/demo');
  });
  it('配置合并保留子项目及开发测试映射，深拷贝不共享引用', () => {
    const project = {
      ...newProject(),
      id: 'group',
      name: '后端组',
      path: '/example/group',
      type: 'backend' as const,
      platforms: [],
      scmDevelopmentProjectId: '123',
      scmTestProjectId: '456',
      subProjects: [
        {
          id: 'repo',
          name: '子项目',
          path: '/example/repo',
          remote: 'origin',
          scmGitId: '1',
          scmModuleName: 'example-repo',
          developmentBranch: 'feature/demo',
          testBranch: 'testing',
        },
      ],
    };
    const merged = mergeConfig(defaultConfig(), { ...defaultConfig(), projects: [project] });
    expect(merged.projects[0]).toEqual(project);
    project.subProjects[0].testBranch = 'changed';
    expect(merged.projects[0].subProjects[0].testBranch).toBe('testing');
  });
});
