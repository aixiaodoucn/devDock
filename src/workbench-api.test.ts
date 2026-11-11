// @vitest-environment happy-dom
import { isProxy, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { api } from './api';
import { defaultConfig, newProject } from './types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));

describe('环境体检与构建历史的桌面接口', () => {
  beforeEach(() => vi.resetAllMocks());

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

  it('授权只通过独立的显式命令请求', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(false);
    expect(await api.requestAccessibilityPermission()).toBe(false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('request_accessibility_permission', undefined);
  });
});
