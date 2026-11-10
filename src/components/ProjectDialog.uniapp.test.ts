// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import { api } from '../api';
import { newProject, type Project, type UniappScript } from '../types';

vi.mock('../api', () => ({
  desktop: true,
  api: { uniappScripts: vi.fn(), nodeVersions: vi.fn(), choosePath: vi.fn(), menus: vi.fn() },
}));
function script(
  id: string,
  title: string,
  platform = 'miniapp',
  uniPlatform = 'mp-weixin',
): UniappScript {
  return {
    id,
    title,
    platform,
    uniPlatform,
    runMenuPath: ['运行', title],
    publishMenuPath: ['发行', '自定义发行', title],
  };
}
async function flush() {
  await Promise.resolve();
  await nextTick();
  await Promise.resolve();
  await nextTick();
}
function mount() {
  const project: Project = { ...newProject(), name: '当前配置项目', path: '/local/target project' };
  const save = vi.fn();
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(ProjectDialog, {
        project,
        tools: { hbuilderxPath: '', terminals: [], jdks: [] },
        hbuilderxPath: '/Applications/HBuilderX.app',
        existing: [],
        saving: false,
        detecting: false,
        onSave: save,
      }),
  });
  app.mount(root);
  const read = async () => {
    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.includes('读取 package.json uni-app.scripts'))!
      .click();
    await flush();
  };
  const select = async (value: string) => {
    const select = root.querySelector<HTMLSelectElement>(
      '[aria-label="从 uni-app.scripts 添加操作"]',
    )!;
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await flush();
  };
  return {
    root,
    project,
    save,
    read,
    select,
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
describe('从目标项目读取 uni-app.scripts', () => {
  beforeEach(() => vi.clearAllMocks());
  it('读取目标目录，为同一脚本导入运行和发行，自动启用平台并保留完整标题', async () => {
    vi.mocked(api.uniappScripts).mockResolvedValue([
      script('wx-test', '微信 / 测试环境'),
      script('h5-test', 'H5 测试', 'web', 'h5'),
      script('native', '原生测试', '', 'app-plus'),
    ]);
    const dialog = mount();
    try {
      await dialog.read();
      expect(api.uniappScripts).toHaveBeenCalledWith(
        expect.objectContaining({ path: '/local/target project' }),
      );
      expect(dialog.root.textContent).not.toContain('读取当前 HBuilderX 菜单');
      expect(
        dialog.root.querySelector<HTMLOptionElement>(
          '[aria-label="从 uni-app.scripts 添加操作"] option[value="4"]',
        )!.disabled,
      ).toBe(true);
      expect(dialog.root.textContent).toContain('暂不支持导入的平台：app-plus');
      await dialog.select('0');
      await dialog.select('1');
      await dialog.select('2');
      await dialog.select('0');
      dialog.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(dialog.save).toHaveBeenCalledOnce();
      const saved: Project = dialog.save.mock.calls[0][0];
      expect(saved.platforms).toEqual(['app', 'miniapp', 'web']);
      expect(saved.commands.map((action) => action.menuPath)).toEqual([
        ['运行', '微信 / 测试环境'],
        ['发行', '自定义发行', '微信 / 测试环境'],
        ['运行', 'H5 测试'],
      ]);
      expect(saved.commands.map((action) => action.name)).toEqual([
        '微信 / 测试环境 · 运行',
        '微信 / 测试环境 · 发行',
        'H5 测试 · 运行',
      ]);
      expect(dialog.project.commands).toEqual([]);
      expect(dialog.root.textContent).toContain('未重复添加');
    } finally {
      dialog.dispose();
    }
  });
  it('同时保留 HBuilderX 活动项目菜单入口并明确来源，微信内置项去除其他项目后缀', async () => {
    vi.mocked(api.uniappScripts).mockResolvedValue([script('target', '目标项目脚本')]);
    vi.mocked(api.menus).mockResolvedValue([
      ['运行', '运行到小程序模拟器', '微信开发者工具 - 其他项目'],
      ['发行', '小程序-微信'],
      ['发行', '网站-H5电脑版'],
    ]);
    const dialog = mount();
    try {
      await dialog.read();
      expect(dialog.root.textContent).toContain(
        '读取的是 HBuilderX 当前激活项目的菜单，不一定是正在配置的项目',
      );
      Array.from(dialog.root.querySelectorAll<HTMLButtonElement>('button'))
        .find((button) => button.textContent?.trim() === '读取 HBuilderX 菜单')!
        .click();
      await flush();
      expect(api.menus).toHaveBeenCalledWith('/Applications/HBuilderX.app');
      expect(
        dialog.root.querySelector('[aria-label="从 uni-app.scripts 添加操作"]'),
      ).not.toBeNull();
      const select = dialog.root.querySelector<HTMLSelectElement>(
        '[aria-label="从 HBuilderX 当前激活项目菜单添加操作"]',
      )!;
      expect(select.textContent).not.toContain('其他项目');
      select.value = '0';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await flush();
      dialog.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(dialog.save.mock.calls[0][0].commands[0]).toMatchObject({
        platform: 'miniapp',
        menuPath: ['运行', '运行到小程序模拟器', '微信开发者工具'],
      });
    } finally {
      dialog.dispose();
    }
  });
  it('切换目录时丢弃旧项目的在途结果，再读取新目录', async () => {
    let resolveOld!: (scripts: UniappScript[]) => void;
    vi.mocked(api.uniappScripts)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
      )
      .mockResolvedValueOnce([script('new', '新项目命令')]);
    const dialog = mount();
    try {
      await dialog.read();
      const path = dialog.root.querySelector<HTMLInputElement>('[aria-label="项目目录"]')!;
      path.value = '/local/new project';
      path.dispatchEvent(new Event('input', { bubbles: true }));
      await flush();
      resolveOld([script('old', '旧项目命令')]);
      await flush();
      expect(dialog.root.textContent).not.toContain('旧项目命令');
      await dialog.read();
      expect(api.uniappScripts).toHaveBeenLastCalledWith(
        expect.objectContaining({ path: '/local/new project' }),
      );
      expect(dialog.root.textContent).toContain('新项目命令');
    } finally {
      dialog.dispose();
    }
  });
  it('失败时清除旧候选，未配置脚本时给出明确空状态', async () => {
    vi.mocked(api.uniappScripts)
      .mockResolvedValueOnce([script('first', '可用脚本')])
      .mockRejectedValueOnce(new Error('无法读取 package.json'))
      .mockResolvedValueOnce([]);
    const dialog = mount();
    try {
      await dialog.read();
      expect(
        dialog.root.querySelector('[aria-label="从 uni-app.scripts 添加操作"]'),
      ).not.toBeNull();
      await dialog.read();
      expect(dialog.root.querySelector('[aria-label="从 uni-app.scripts 添加操作"]')).toBeNull();
      expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain(
        '无法读取 package.json',
      );
      await dialog.read();
      expect(dialog.root.textContent).toContain('未配置 uni-app.scripts');
      expect(dialog.root.querySelector('[role="alert"]')).toBeNull();
    } finally {
      dialog.dispose();
    }
  });
});
