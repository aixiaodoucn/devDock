import { invoke, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { AppConfig, Project, ManifestInfo, Tools, Action, Settings, BuildMode } from './types';
import { defaultConfig } from './types';

export const desktop = isTauri();
// 浏览器预览使用独立本地存储；构建和外部程序调用始终只在桌面应用中执行。
const PREVIEW_KEY = 'dev-dock-preview-v1';
export const api = {
  async load(): Promise<AppConfig> {
    if (desktop) return invoke('load_config');
    const raw = localStorage.getItem(PREVIEW_KEY) ?? localStorage.getItem('ai-dev-preview-v1');
    const config = raw ? JSON.parse(raw) : defaultConfig();
    // 仅首次读取时复制旧预览配置，保留旧值且不覆盖已有的新配置。
    if (raw && localStorage.getItem(PREVIEW_KEY) === null) {
      localStorage.setItem(PREVIEW_KEY, raw);
    }
    // 旧版浏览器预览配置没有主题和目录开关，补默认值而不改动项目或工具路径。
    config.settings = { ...defaultConfig().settings, ...config.settings };
    return config;
  },
  async persist(config: AppConfig) {
    if (desktop) await invoke('save_config', { config });
    else localStorage.setItem(PREVIEW_KEY, JSON.stringify(config));
  },
  manifest(project: Project) {
    return native<ManifestInfo>('read_manifest', { project });
  },
  detect() {
    return native<Tools>('detect_tools');
  },
  packageScripts(project: Project) {
    return native<{ name: string; command: string }[]>('read_package_scripts', { project });
  },
  startBuild(project: Project, mode: BuildMode, versionName: string, versionCode: string) {
    return native<string>('start_build', { project, mode, versionName, versionCode });
  },
  runAction(project: Project, action: Action, settings: Settings) {
    return native<string>('run_action', { project, action, settings });
  },
  cancel(taskId: string) {
    return native<void>('cancel_task', { taskId });
  },
  openPath(path: string) {
    return native<void>('open_path', { path });
  },
  menus(hbuilderxPath: string) {
    return native<string[][]>('list_menus', { hbuilderxPath });
  },
  recover() {
    return native<string[]>('recover_pending');
  },
  async choosePath(directory = true): Promise<string | null> {
    if (!desktop) throw new Error('文件选择需要在桌面应用中使用；预览时可手动填写路径。');
    const selected = await open({ directory, multiple: false });
    return typeof selected === 'string' ? selected : null;
  },
  async exportBackup(config: AppConfig): Promise<boolean> {
    if (!desktop) {
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(config, null, 2)], { type: 'application/json' }),
      );
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`;
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      return true;
    }
    const path = await save({
      defaultPath: `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: 'JSON 备份', extensions: ['json'] }],
    });
    if (!path) return false;
    await invoke('write_backup', { path, config });
    return true;
  },
  async importBackup(): Promise<AppConfig | null> {
    if (!desktop) throw new Error('请在桌面应用中导入备份，以完成配置校验。');
    const path = await open({
      multiple: false,
      filters: [{ name: 'JSON 备份', extensions: ['json'] }],
    });
    if (typeof path !== 'string') return null;
    return invoke('read_backup', { path });
  },
};
function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!desktop)
    return Promise.reject(new Error('当前是浏览器预览，请在 DevDock 桌面应用中执行此操作。'));
  return invoke<T>(command, args);
}
