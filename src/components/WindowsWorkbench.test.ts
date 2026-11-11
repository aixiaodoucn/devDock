// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import SettingsPage from './SettingsPage.vue';
import { api } from '../api';
import { defaultConfig, newProject, type Project } from '../types';

vi.mock('../platform', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../platform')>()),
  isWindows: true,
}));
vi.mock('../api', () => ({
  desktop: true,
  api: { nodeVersions: vi.fn(), choosePath: vi.fn(), menus: vi.fn() },
}));
async function flush() {
  await Promise.resolve();
  await nextTick();
  await Promise.resolve();
}
function mount(component: Parameters<typeof createApp>[0]) {
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp(component);
  app.mount(root);
  return {
    root,
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
const tools = {
  hbuilderxPath: '',
  terminals: [{ name: 'Windows PowerShell', path: 'PowerShell' }],
  jdks: [],
};
describe('Windows 基础工作台界面', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.nodeVersions).mockResolvedValue([]);
    vi.mocked(api.choosePath).mockResolvedValue(null);
  });
  it('Windows 新设置不使用 Mac 应用路径，工具选择器选择 exe 文件', async () => {
    const settings = defaultConfig().settings;
    expect(settings.terminal).toBe('PowerShell');
    expect(settings.hbuilderxPath).toBe('');
    const panel = mount({
      render: () =>
        h(SettingsPage, {
          settings,
          tools,
          detecting: false,
          saving: false,
          busy: false,
          projectCount: 0,
        }),
    });
    try {
      panel.root
        .querySelector<HTMLButtonElement>('.settings-tools-grid > .panel .path-input button')!
        .click();
      await flush();
      expect(api.choosePath).toHaveBeenLastCalledWith(false);
      panel.root
        .querySelector<HTMLButtonElement>('.settings-terminal-fields .path-input button')!
        .click();
      await flush();
      expect(api.choosePath).toHaveBeenLastCalledWith(false);
      expect(panel.root.textContent).toContain('Windows 本地执行');
    } finally {
      panel.dispose();
    }
  });
  it('支持 Windows 中文空格项目目录，并提示版本切换只影响新会话', async () => {
    const project: Project = {
      ...newProject(),
      name: 'Windows 项目',
      type: 'nodejs',
      path: String.raw`C:\工作目录\项目 Demo`,
      platforms: ['web'],
    };
    const save = vi.fn();
    const panel = mount({
      render: () =>
        h(ProjectDialog, {
          project,
          tools,
          existing: [],
          saving: false,
          detecting: false,
          onSave: save,
        }),
    });
    try {
      await flush();
      expect(panel.root.textContent).toContain('不调用 nvm use');
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save).toHaveBeenCalledWith(expect.objectContaining({ path: project.path }));
    } finally {
      panel.dispose();
    }
  });
  it('Windows 不可触发 HBuilderX 菜单扫描，项目脚本读取仍可配置', async () => {
    const project = { ...newProject(), name: 'Windows uni-app', path: String.raw`D:\uniapp\demo` };
    const panel = mount({
      render: () =>
        h(ProjectDialog, { project, tools, existing: [], saving: false, detecting: false }),
    });
    try {
      const buttons = Array.from(panel.root.querySelectorAll<HTMLButtonElement>('button'));
      const hbuilder = buttons.find(
        (button) => button.textContent?.trim() === '读取 HBuilderX 菜单',
      )!;
      expect(hbuilder.disabled).toBe(true);
      hbuilder.click();
      expect(api.menus).not.toHaveBeenCalled();
      expect(
        buttons.find((button) => button.textContent?.includes('读取 package.json uni-app.scripts'))!
          .disabled,
      ).toBe(false);
    } finally {
      panel.dispose();
    }
  });
});
