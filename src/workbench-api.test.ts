// @vitest-environment happy-dom
import { isProxy, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { downloadDir, join } from '@tauri-apps/api/path';
import { save } from '@tauri-apps/plugin-dialog';
import { api } from './api';
import { defaultConfig, newProject } from './types';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/path', () => ({ downloadDir: vi.fn(), join: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

describe('环境体检与构建历史的桌面接口', () => {
  beforeEach(() => vi.resetAllMocks());

  it('导出默认定位系统下载目录，实际写入用户选定的位置', async () => {
    const config = defaultConfig();
    const filename = `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`;
    vi.mocked(downloadDir).mockResolvedValue('/example/Downloads');
    vi.mocked(join).mockResolvedValue(`/example/Downloads/${filename}`);
    vi.mocked(save).mockResolvedValue('/example/selected/backup.json');
    expect(await api.exportBackup(config)).toBe(true);
    expect(join).toHaveBeenCalledWith('/example/Downloads', filename);
    expect(save).toHaveBeenCalledWith({
      defaultPath: `/example/Downloads/${filename}`,
      filters: [{ name: 'JSON 备份', extensions: ['json'] }],
    });
    expect(invoke).toHaveBeenCalledWith('write_backup', {
      path: '/example/selected/backup.json',
      config,
    });
  });

  it('下载目录不可用时仍显示保存对话框，取消不写入', async () => {
    vi.mocked(downloadDir).mockRejectedValue(new Error('下载目录不可用'));
    vi.mocked(save).mockResolvedValue(null);
    expect(await api.exportBackup(defaultConfig())).toBe(false);
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        defaultPath: `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`,
      }),
    );
    expect(invoke).not.toHaveBeenCalled();
  });

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
