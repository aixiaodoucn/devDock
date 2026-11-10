// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import QuickPanel from './QuickPanel.vue';
import { api } from './api';
import { defaultConfig, newProject, type Project } from './types';

const handlers = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock('./api', () => ({
  desktop: true,
  api: {
    load: vi.fn(),
    runAction: vi.fn(),
    showMain: vi.fn(),
    hideQuickPanel: vi.fn(),
    openProjectTerminal: vi.fn(),
    openProjectCommand: vi.fn(),
  },
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    handlers.set(name, handler);
    return () => {
      handlers.delete(name);
    };
  }),
}));
const node: Project = {
  ...newProject(),
  id: 'node',
  name: '管理后台',
  type: 'nodejs',
  path: '/tmp/admin',
  platforms: ['web'],
  nodeVersion: 'v16.20.2',
  nvmDir: '/tmp/nvm',
  commands: [
    {
      id: 'dev',
      name: '开发服务',
      platform: 'web',
      kind: 'shell',
      menuPath: [],
      command: 'npm run dev',
    },
  ],
};
const uni: Project = {
  ...newProject(),
  id: 'uni',
  name: '门店小程序',
  path: '/tmp/shop',
  platforms: ['miniapp'],
  commands: [
    {
      id: 'weixin',
      name: '微信开发',
      platform: 'miniapp',
      kind: 'menu',
      menuPath: ['运行', '运行到小程序模拟器', '微信开发者工具'],
      command: '',
    },
  ],
};
const config = { ...defaultConfig(), projects: [node, uni] };
async function flush() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
  await nextTick();
}
async function mount() {
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({ render: () => h(QuickPanel) });
  app.mount(root);
  await flush();
  return {
    root,
    dispose: () => {
      app.unmount();
      root.remove();
    },
  };
}
describe('菜单栏快捷弹框', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    handlers.clear();
    vi.mocked(api.load).mockResolvedValue(config);
    vi.mocked(api.runAction).mockResolvedValue('task-1');
    vi.mocked(api.openProjectTerminal).mockResolvedValue('task-1');
    vi.mocked(api.openProjectCommand).mockResolvedValue('task-2');
    vi.mocked(api.showMain).mockResolvedValue();
    vi.mocked(api.hideQuickPanel).mockResolvedValue();
  });
  it('读取已配置项目，并按命令名称搜索和展开对应项目', async () => {
    const panel = await mount();
    try {
      expect(panel.root.querySelectorAll('.quick-project')).toHaveLength(2);
      const input = panel.root.querySelector('input')!;
      input.value = '微信开发';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      expect(panel.root.querySelectorAll('.quick-project')).toHaveLength(1);
      expect(panel.root.querySelector('.quick-action')?.textContent).toContain('微信开发');
      expect(panel.root.querySelector('.quick-action')?.getAttribute('title')).toBe(
        '运行 → 运行到小程序模拟器 → 微信开发者工具',
      );
    } finally {
      panel.dispose();
    }
  });
  it('折叠箭头右侧提供目录与执行两个入口，项目命令优先于全局默认', async () => {
    const privateProject = { ...node, aiToolId: 'claude' };
    vi.mocked(api.load).mockResolvedValue({
      ...config,
      projects: [privateProject, uni],
      settings: {
        ...config.settings,
        defaultAiToolId: 'git',
        aiTools: [
          { id: 'git', name: '拉取代码', command: 'git pull', enabled: true },
          { id: 'claude', name: 'Claude Code', command: 'claude', enabled: true },
        ],
      },
    });
    const panel = await mount();
    try {
      const rows = panel.root.querySelectorAll('.quick-project-header');
      expect(
        rows[0]
          .querySelector('.quick-project-toggle')
          ?.nextElementSibling?.classList.contains('quick-open-terminal'),
      ).toBe(true);
      expect(rows[0].querySelector('.quick-run-command')?.getAttribute('title')).toContain(
        'claude',
      );
      expect(rows[1].querySelector('.quick-run-command')?.getAttribute('title')).toContain(
        'git pull',
      );
      rows[0].querySelector<HTMLButtonElement>('.quick-open-terminal')!.click();
      await flush();
      expect(api.openProjectTerminal).toHaveBeenCalledWith(privateProject);
      expect(api.openProjectCommand).not.toHaveBeenCalled();
      handlers.get('task-event')?.({
        payload: {
          taskId: 'task-1',
          projectId: node.id,
          kind: 'status',
          status: 'triggered',
          message: '已触发',
        },
      });
      await flush();
      rows[0].querySelector<HTMLButtonElement>('.quick-run-command')!.click();
      await flush();
      expect(api.openProjectCommand).toHaveBeenCalledWith(privateProject);
    } finally {
      panel.dispose();
    }
  });
  it('未选择可用命令时，快捷执行入口明确提示仅打开目录', async () => {
    const panel = await mount();
    try {
      expect(panel.root.querySelector('.quick-run-command')?.getAttribute('title')).toContain(
        '仅打开项目目录',
      );
      panel.root.querySelector<HTMLButtonElement>('.quick-run-command')!.click();
      await flush();
      expect(api.openProjectCommand).toHaveBeenCalledWith(node);
      expect(api.runAction).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('快捷执行复用项目完整配置，包括所选 nvm 版本', async () => {
    const panel = await mount();
    try {
      panel.root.querySelector<HTMLButtonElement>('.quick-action')!.click();
      await flush();
      expect(api.runAction).toHaveBeenCalledWith(node, node.commands[0], config.settings);
    } finally {
      panel.dispose();
    }
  });
  it('快捷停止派生同一项目的停止菜单，且终端命令不显示停止入口', async () => {
    const panel = await mount();
    try {
      expect(panel.root.querySelector('.quick-stop')).toBeNull();
      const input = panel.root.querySelector('input')!;
      input.value = '微信开发';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('.quick-stop')!.click();
      await flush();
      expect(api.runAction).toHaveBeenCalledWith(
        uni,
        expect.objectContaining({
          id: uni.commands[0].id,
          name: '停止 · 微信开发',
          menuPath: ['运行', '运行到小程序模拟器', '停止微信开发者工具'],
        }),
        config.settings,
      );
      expect(uni.commands[0].menuPath).toEqual(['运行', '运行到小程序模拟器', '微信开发者工具']);
    } finally {
      panel.dispose();
    }
  });
  it('从项目入口打开主窗口时传递项目 ID', async () => {
    const panel = await mount();
    try {
      panel.root.querySelector<HTMLButtonElement>('.quick-open-project')!.click();
      await flush();
      expect(api.showMain).toHaveBeenCalledWith('node');
    } finally {
      panel.dispose();
    }
  });
  it('顶部打开入口和 Esc 隐藏使用原生窗口命令', async () => {
    const panel = await mount();
    try {
      panel.root.querySelector<HTMLButtonElement>('.quick-header .button')!.click();
      await flush();
      expect(api.showMain).toHaveBeenCalledWith(undefined);
      panel.root
        .querySelector('input')!
        .dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      await flush();
      expect(api.hideQuickPanel).toHaveBeenCalledTimes(1);
    } finally {
      panel.dispose();
    }
  });
  it('配置读取失败时显示错误，不显示可执行的项目命令', async () => {
    vi.mocked(api.load).mockRejectedValue(new Error('配置损坏'));
    const panel = await mount();
    try {
      expect(panel.root.textContent).toContain('配置损坏');
      expect(panel.root.querySelector('.quick-action')).toBeNull();
      expect(api.runAction).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('项目占用提示不会被通用任务结束状态覆盖', async () => {
    vi.mocked(api.runAction).mockImplementation(async () => {
      handlers.get('task-event')?.({
        payload: {
          taskId: 'occupied',
          projectId: 'node',
          kind: 'notice',
          message: '有项目「shopopening」已经在运行了；当前项目未启动',
        },
      });
      handlers.get('task-event')?.({
        payload: {
          taskId: 'occupied',
          projectId: 'node',
          kind: 'status',
          status: 'triggered',
          message: '任务结束',
        },
      });
      return 'occupied';
    });
    const panel = await mount();
    try {
      panel.root.querySelector<HTMLButtonElement>('.quick-action')!.click();
      await flush();
      expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('shopopening');
      expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('当前项目未启动');
    } finally {
      panel.dispose();
    }
  });
  it('不会遗漏 invoke 返回前已到达的任务失败事件', async () => {
    vi.mocked(api.runAction).mockImplementation(async () => {
      handlers.get('task-event')?.({
        payload: {
          taskId: 'early',
          projectId: 'node',
          kind: 'status',
          status: 'error',
          message: 'Node 版本不存在',
        },
      });
      return 'early';
    });
    const panel = await mount();
    try {
      panel.root.querySelector<HTMLButtonElement>('.quick-action')!.click();
      await flush();
      expect(panel.root.querySelector('[role="status"]')?.textContent).toContain('Node 版本不存在');
      expect(panel.root.querySelector<HTMLButtonElement>('.quick-action')!.disabled).toBe(false);
    } finally {
      panel.dispose();
    }
  });
  it('收到配置更新后重新读取，卸载时取消订阅', async () => {
    const panel = await mount();
    try {
      vi.mocked(api.load).mockResolvedValue({ ...config, projects: [uni] });
      handlers.get('config-updated')?.({ payload: null });
      await flush();
      expect(panel.root.querySelectorAll('.quick-project')).toHaveLength(1);
      expect(panel.root.querySelector('.quick-project-title')?.textContent).toContain('门店小程序');
    } finally {
      panel.dispose();
    }
    expect(handlers.size).toBe(0);
  });
});
