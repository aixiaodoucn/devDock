// @vitest-environment happy-dom
import { isProxy, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { api } from './api';
import { defaultConfig, newProject } from './types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));

describe('环境体检与构建历史的桌面接口', () => {
  beforeEach(() => vi.resetAllMocks());

  it('Node 检测兼容无参请求并支持显式管理器与自定义目录', async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    await api.nodeVersions();
    expect(invoke).toHaveBeenLastCalledWith('detect_node_versions', undefined);
    await api.nodeVersions({ manager: 'fnm', dir: ' /example/custom fnm ' });
    expect(invoke).toHaveBeenLastCalledWith('detect_node_versions', {
      manager: 'fnm',
      dir: '/example/custom fnm',
    });
    await api.nodeVersions({ manager: 'nvm', dir: '' });
    expect(invoke).toHaveBeenLastCalledWith('detect_node_versions', { manager: 'nvm', dir: null });
  });

  it('体检发送独立快照，Vue 响应式对象不进入原生参数', async () => {
    const project = reactive(newProject());
    project.id = 'example-project';
    const settings = reactive(defaultConfig().settings);
    vi.mocked(invoke).mockResolvedValueOnce({ projectId: project.id, checkedAt: 1, items: [] });
    await api.checkEnvironment(project, settings);
    const [command, args] = vi.mocked(invoke).mock.calls[0];
    const payload = args as { project: typeof project; settings: typeof settings };
    expect(command).toBe('check_environment');
    expect(payload.project).toEqual(project);
    expect(isProxy(payload.project)).toBe(false);
    expect(isProxy(payload.project.commands)).toBe(false);
    expect(isProxy(payload.settings)).toBe(false);
    project.name = '修改后的项目';
    expect(payload.project.name).not.toBe(project.name);
  });

  it('历史查询和移除始终携带当前项目标识', async () => {
    vi.mocked(invoke).mockResolvedValue([]);
    await api.buildHistory('example-project');
    expect(invoke).toHaveBeenLastCalledWith('list_build_history', { projectId: 'example-project' });
    await api.deleteBuildHistory('example-project', 'example-record');
    expect(invoke).toHaveBeenLastCalledWith('delete_build_history', {
      projectId: 'example-project',
      id: 'example-record',
    });
  });

  it('历史保留设置通过原生接口持久化并返回实际清理数量', async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ retentionLimit: 100 });
    expect(await api.buildHistorySettings()).toEqual({ retentionLimit: 100 });
    expect(invoke).toHaveBeenLastCalledWith('get_build_history_settings', undefined);
    vi.mocked(invoke).mockResolvedValueOnce({ retentionLimit: 50, removedCount: 12 });
    expect(await api.saveBuildHistorySettings(50)).toEqual({
      retentionLimit: 50,
      removedCount: 12,
    });
    expect(invoke).toHaveBeenLastCalledWith('save_build_history_settings', { retentionLimit: 50 });
  });

  it('授权只通过独立的显式命令请求', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(false);
    expect(await api.requestAccessibilityPermission()).toBe(false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('request_accessibility_permission', undefined);
  });
});
