import { describe, expect, it } from 'vitest';
import {
  defaultConfig,
  mergeConfig,
  newProject,
  normalizeProjectPlatforms,
  projectNodeManager,
} from './types';

describe('Node 环境兼容', () => {
  it('旧 nvm 版本与无版本项目分别保留 nvm 和终端默认含义', () => {
    const legacy = { ...newProject(), nodeVersion: 'v22.12.0', nvmDir: '/example/nvm' };
    delete legacy.nodeManager;
    delete legacy.fnmDir;
    const restored = normalizeProjectPlatforms(legacy);
    expect(projectNodeManager(restored)).toBe('nvm');
    expect(restored.nodeVersion).toBe('v22.12.0');
    expect(restored.nvmDir).toBe('/example/nvm');
    expect(projectNodeManager(newProject())).toBe('system');
    expect(projectNodeManager({ ...newProject(), nodeManager: 'fnm' })).toBe('system');
  });
  it('公共模式优先于旧项目来源，无版本时仍使用终端默认', () => {
    const project = { ...newProject(), nodeVersion: 'v22.12.0', nvmDir: '/example/nvm' };
    expect(projectNodeManager(project, { nodeManager: 'fnm' })).toBe('fnm');
    expect(projectNodeManager(project, { nodeManager: 'system' })).toBe('system');
    expect(projectNodeManager({ ...project, nodeVersion: '' }, { nodeManager: 'fnm' })).toBe(
      'system',
    );
    expect(projectNodeManager(project, { nodeManager: '' })).toBe('nvm');
  });
  it('公共选择的项目仅保存版本，合并导入保留当前电脑公共设置', () => {
    const current = defaultConfig();
    current.settings.nodeManager = 'fnm';
    current.settings.fnmDir = '/example/local-fnm';
    const incoming = defaultConfig();
    incoming.settings.nodeManager = 'nvm';
    incoming.settings.nvmDir = '/example/other-device-nvm';
    incoming.projects = [
      {
        ...newProject(),
        id: 'node',
        type: 'nodejs',
        path: '/example/node',
        nodeManager: 'inherit',
        nodeVersion: 'v22.12.0',
      },
    ];
    const result = mergeConfig(current, incoming);
    expect(result.settings).toEqual(current.settings);
    expect(result.projects[0]).toMatchObject({
      nodeManager: 'inherit',
      nodeVersion: 'v22.12.0',
      nvmDir: '',
      fnmDir: '',
    });
    expect(projectNodeManager(result.projects[0], result.settings)).toBe('fnm');
  });
  it('fnm 来源和数据目录在配置备份合并后保持独立', () => {
    const incoming = defaultConfig();
    incoming.projects = [
      {
        ...newProject(),
        id: 'node',
        type: 'nodejs',
        path: '/example/node',
        nodeManager: 'fnm',
        nodeVersion: 'v22.12.0',
        fnmDir: '/example/fnm data',
      },
    ];
    const result = mergeConfig(defaultConfig(), JSON.parse(JSON.stringify(incoming)));
    expect(projectNodeManager(result.projects[0])).toBe('fnm');
    expect(result.projects[0].fnmDir).toBe('/example/fnm data');
    expect(result.projects[0].nvmDir).toBe('');
    expect(result.schemaVersion).toBe(1);
  });
});

describe('配置备份合并', () => {
  it('旧平台合并为 Web 和小程序，保留全部操作及独立菜单路径', () => {
    const project = {
      ...newProject(),
      platforms: ['app', 'weixin', 'dingtalk', 'pc'],
      commands: [
        {
          id: 'wx',
          name: '微信开发',
          platform: 'weixin',
          kind: 'menu' as const,
          command: '',
          envVars: '',
          menuPath: ['运行', '微信开发者工具'],
        },
        {
          id: 'ding',
          name: '钉钉开发',
          platform: 'dingtalk',
          kind: 'menu' as const,
          command: '',
          envVars: '',
          menuPath: ['运行', '钉钉小程序-开发'],
        },
        {
          id: 'web',
          name: 'Web 开发',
          platform: 'pc',
          kind: 'shell' as const,
          command: 'pnpm dev',
          envVars: '',
          menuPath: [],
        },
      ],
    };
    const normalized = normalizeProjectPlatforms(project);
    expect(normalized.platforms).toEqual(['app', 'miniapp', 'web']);
    expect(normalized.commands.map((action) => action.platform)).toEqual([
      'miniapp',
      'miniapp',
      'web',
    ]);
    expect(normalized.commands.map((action) => action.menuPath)).toEqual(
      project.commands.map((action) => action.menuPath),
    );
    expect(normalized.commands.map((action) => action.id)).toEqual(['wx', 'ding', 'web']);
    const current = { ...defaultConfig(), projects: [project] };
    const merged = mergeConfig(current, defaultConfig());
    expect(merged.projects[0]).toEqual(normalized);
    merged.projects[0].commands[0].menuPath.push('changed');
    expect(project.commands[0].menuPath).toEqual(['运行', '微信开发者工具']);
    expect(project.platforms).toEqual(['app', 'weixin', 'dingtalk', 'pc']);
  });
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
      envVars: '',
      menuPath: [],
    });
    expect(incoming.projects[0].commands).toEqual([]);
  });
});
