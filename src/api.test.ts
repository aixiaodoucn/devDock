// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { defaultConfig, newProject } from './types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => false, invoke: vi.fn() }));

describe('DevDock 预览配置迁移', () => {
  beforeEach(() => localStorage.clear());

  it('首次载入复制旧配置并保留原记录', async () => {
    const config = defaultConfig();
    config.settings.jdkPath = '/local/jdk';
    const raw = JSON.stringify(config);
    localStorage.setItem('ai-dev-preview-v1', raw);
    expect(await api.load()).toEqual(config);
    expect(localStorage.getItem('dev-dock-preview-v1')).toBe(raw);
    expect(localStorage.getItem('ai-dev-preview-v1')).toBe(raw);
  });

  it('已有新版配置时不被旧版覆盖', async () => {
    const config = defaultConfig();
    config.settings.jdkPath = '/new/jdk';
    localStorage.setItem('dev-dock-preview-v1', JSON.stringify(config));
    localStorage.setItem('ai-dev-preview-v1', JSON.stringify(defaultConfig()));
    expect(await api.load()).toEqual(config);
  });

  it('缺少新增偏好的旧配置补默认值并保留工具路径', async () => {
    localStorage.setItem(
      'dev-dock-preview-v1',
      JSON.stringify({
        schemaVersion: 1,
        projects: [],
        settings: {
          hbuilderxPath: '/local/HBuilderX.app',
          terminal: 'Terminal',
          jdkPath: '/local/jdk8',
        },
      }),
    );
    const config = await api.load();
    expect(config.settings).toMatchObject({
      theme: 'lavender',
      openOutputAfterBuild: true,
      aiTools: [],
      defaultAiToolId: '',
      jdkPath: '/local/jdk8',
      hbuilderxPath: '/local/HBuilderX.app',
    });
  });

  it('旧项目缺少 Node 版本字段时沿用终端默认，不覆盖已有操作', async () => {
    const project = {
      ...newProject(),
      name: '旧 Node 项目',
      path: '/local/node-app',
      type: 'nodejs',
      platforms: ['pc'],
    };
    const config = JSON.parse(JSON.stringify({ ...defaultConfig(), projects: [project] }));
    delete config.projects[0].nodeVersion;
    delete config.projects[0].nvmDir;
    delete config.projects[0].aiToolId;
    localStorage.setItem('dev-dock-preview-v1', JSON.stringify(config));
    expect((await api.load()).projects[0]).toMatchObject({
      nodeVersion: '',
      nvmDir: '',
      aiToolId: '',
      path: '/local/node-app',
      commands: [],
    });
  });

  it('无效旧数据不复制为新版配置', async () => {
    localStorage.setItem('ai-dev-preview-v1', '{');
    await expect(api.load()).rejects.toThrow();
    expect(localStorage.getItem('dev-dock-preview-v1')).toBeNull();
  });
});
