// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { defaultConfig } from './types';

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
      jdkPath: '/local/jdk8',
      hbuilderxPath: '/local/HBuilderX.app',
    });
  });

  it('无效旧数据不复制为新版配置', async () => {
    localStorage.setItem('ai-dev-preview-v1', '{');
    await expect(api.load()).rejects.toThrow();
    expect(localStorage.getItem('dev-dock-preview-v1')).toBeNull();
  });
});
