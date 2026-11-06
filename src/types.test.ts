import { describe, expect, it } from 'vitest';
import { defaultConfig, mergeConfig, newProject } from './types';

describe('配置备份合并', () => {
  it('更新同目录项目，保留本机项目 ID 与工具设置', () => {
    const current = defaultConfig();
    const project = { ...newProject(), id: 'local', name: '原项目', path: '/workspace/shop' };
    current.projects = [project];
    current.settings.terminal = '/Applications/Terminal.app';
    const incoming = defaultConfig();
    incoming.projects = [{ ...project, id: 'remote', name: '备份项目', path: '/workspace/shop/' }];
    const result = mergeConfig(current, incoming);
    expect(result.projects).toHaveLength(1);
    expect(result.projects[0]).toMatchObject({ id: 'local', name: '备份项目' });
    expect(result.settings.terminal).toBe('/Applications/Terminal.app');
    expect(current.projects[0].name).toBe('原项目');
  });

  it('ID 与路径指向不同现有项目时拒绝覆盖', () => {
    const current = defaultConfig();
    current.projects = [
      { ...newProject(), id: 'a', path: '/a' },
      { ...newProject(), id: 'b', path: '/b' },
    ];
    const incoming = defaultConfig();
    incoming.projects = [{ ...current.projects[0], path: '/b' }];
    expect(() => mergeConfig(current, incoming)).toThrow('分别对应不同');
    expect(current.projects.map((project) => project.path)).toEqual(['/a', '/b']);
  });

  it('同 ID 允许迁移目录，独立项目保持顺序并追加', () => {
    const current = defaultConfig();
    current.projects = [
      { ...newProject(), id: 'a', name: 'A', path: '/old/a' },
      { ...newProject(), id: 'b', name: 'B', path: '/old/b' },
    ];
    const incoming = defaultConfig();
    incoming.projects = [
      { ...current.projects[0], path: '/new/a' },
      { ...newProject(), id: 'c', name: 'C', path: '/new/c' },
    ];
    const result = mergeConfig(current, incoming);
    expect(result.projects.map((project) => project.id)).toEqual(['a', 'b', 'c']);
    expect(result.projects[0].path).toBe('/new/a');
    result.projects[0].commands.push({
      id: 'cmd',
      name: '开发',
      kind: 'shell',
      platform: 'pc',
      command: 'pnpm dev',
      menuPath: [],
    });
    expect(incoming.projects[0].commands).toEqual([]);
  });
});
