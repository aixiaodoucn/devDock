// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsPage from './SettingsPage.vue';
import { api } from '../api';
import { defaultConfig, type NodeVersion, type Settings } from '../types';

vi.mock('../api', () => ({
  desktop: true,
  api: { choosePath: vi.fn(), nodeVersions: vi.fn() },
}));
async function flush() {
  await Promise.resolve();
  await nextTick();
  await Promise.resolve();
}
function mount(settings: Settings = defaultConfig().settings) {
  const state = reactive({ settings, saving: false, busy: false });
  const detectTools = vi.fn();
  const save = vi.fn();
  const exportSettings = vi.fn();
  const error = vi.fn();
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(SettingsPage, {
        ...state,
        tools: { hbuilderxPath: '', terminals: [], jdks: [] },
        detecting: false,
        projectCount: 0,
        onDetect: detectTools,
        onSave: save,
        onExport: exportSettings,
        onError: error,
      }),
  });
  app.mount(root);
  return {
    state,
    root,
    detectTools,
    save,
    exportSettings,
    error,
    submit: () =>
      root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })),
    export: () =>
      Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
        .find((button) => button.textContent?.includes('导出 JSON'))!
        .click(),
    detect: () =>
      Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
        .find(
          (button) =>
            button.textContent?.includes('检测本地目录') || button.textContent?.includes('检测中…'),
        )!
        .click(),
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
async function change(root: HTMLElement, selector: string, value: string) {
  const field = root.querySelector<HTMLInputElement | HTMLSelectElement>(selector)!;
  field.value = value;
  field.dispatchEvent(
    new Event(field instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }),
  );
  await flush();
}
const rootSelector = '[aria-label="公共 Node.js 版本数据目录"]';
function version(manager: 'nvm' | 'fnm', dir: string): NodeVersion {
  return {
    manager,
    version: 'v22.0.0',
    path: `${dir}/example-node`,
    nvmDir: manager === 'nvm' ? dir : '',
    ...(manager === 'fnm' ? { fnmDir: dir } : {}),
  };
}
describe('SettingsPage 公共 Node.js 环境', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.nodeVersions).mockResolvedValue([]);
    vi.mocked(api.choosePath).mockResolvedValue(null);
  });
  it('自动检测同时检测工具与所选 Node 管理器，顶部保存提交检测后的草稿', async () => {
    vi.mocked(api.nodeVersions).mockResolvedValue([version('fnm', '/example/fnm')]);
    const panel = mount({ ...defaultConfig().settings, nodeManager: 'fnm' });
    try {
      const actions = panel.root.querySelector('.settings-tool-actions')!;
      const buttons = actions.querySelectorAll<HTMLButtonElement>('button');
      buttons[0].click();
      expect(panel.detectTools).toHaveBeenCalledTimes(1);
      expect(api.nodeVersions).toHaveBeenCalledWith({ manager: 'fnm' });
      panel.submit();
      expect(panel.save).not.toHaveBeenCalled();
      await flush();
      expect(buttons[1].textContent).toContain('保存设置');
      buttons[1].click();
      expect(panel.save).toHaveBeenCalledWith(
        expect.objectContaining({ nodeManager: 'fnm', fnmDir: '/example/fnm' }),
      );
      expect(panel.state.settings.fnmDir).toBe('');
    } finally {
      panel.dispose();
    }
  });
  it.each(['', 'system'] as const)(
    '自动检测在模式 %s 下发现管理器但不覆盖选择',
    async (manager) => {
      vi.mocked(api.nodeVersions).mockResolvedValue([
        version('nvm', '/example/nvm'),
        version('fnm', '/example/fnm'),
      ]);
      const panel = mount({ ...defaultConfig().settings, nodeManager: manager });
      try {
        panel.root.querySelector<HTMLButtonElement>('.settings-tool-actions button')!.click();
        await flush();
        expect(panel.detectTools).toHaveBeenCalledTimes(1);
        expect(api.nodeVersions).toHaveBeenCalledWith(undefined);
        expect(panel.root.textContent).toContain('已检测到 nvm、fnm，共 2 个 Node.js 版本');
        panel.submit();
        expect(panel.save).toHaveBeenCalledWith(expect.objectContaining({ nodeManager: manager }));
      } finally {
        panel.dispose();
      }
    },
  );
  it.each([undefined, ''] as const)(
    '旧配置 %s 在保存其它偏好时保持兼容，不自动启用覆盖',
    async (manager) => {
      const settings = defaultConfig().settings;
      if (manager === undefined) delete settings.nodeManager;
      else settings.nodeManager = manager;
      const panel = mount(settings);
      try {
        expect(panel.root.querySelector<HTMLSelectElement>('#settings-node-manager')!.value).toBe(
          '',
        );
        expect(panel.root.textContent).toContain('未设置（保留已有项目配置）');
        await change(panel.root, '[aria-label="默认 JDK 路径"]', '/example/jdk');
        panel.submit();
        panel.export();
        expect(panel.save.mock.calls[0][0].nodeManager).toBe(manager);
        expect(panel.exportSettings.mock.calls[0][0].nodeManager).toBe(manager);
        expect(Object.hasOwn(panel.save.mock.calls[0][0], 'nodeManager')).toBe(
          manager !== undefined,
        );
        expect(api.nodeVersions).not.toHaveBeenCalled();
      } finally {
        panel.dispose();
      }
    },
  );
  it('system 无需目录，fnm 自定义绝对目录保存及导出，草稿不污染 props', async () => {
    const panel = mount();
    try {
      await change(panel.root, '#settings-node-manager', 'system');
      panel.submit();
      expect(panel.save).toHaveBeenLastCalledWith(
        expect.objectContaining({ nodeManager: 'system' }),
      );
      expect(panel.root.querySelector(rootSelector)).toBeNull();
      await change(panel.root, '#settings-node-manager', 'fnm');
      await change(panel.root, rootSelector, '/example/custom fnm');
      panel.submit();
      panel.export();
      const expected = expect.objectContaining({
        nodeManager: 'fnm',
        fnmDir: '/example/custom fnm',
      });
      expect(panel.save).toHaveBeenLastCalledWith(expected);
      expect(panel.exportSettings).toHaveBeenCalledWith(expected);
      expect(panel.state.settings.fnmDir || '').toBe('');
      expect(panel.state.settings.nodeManager || '').toBe('');
    } finally {
      panel.dispose();
    }
  });
  it.each([
    ['unknown', '/example/root'],
    ['nvm', ''],
    ['fnm', ''],
    ['nvm', 'relative/root'],
    ['fnm', '~/fnm'],
    ['fnm', 'C:relative'],
  ])('无效模式或目录拦截保存与备份：%s %s', async (manager, dir) => {
    const settings = {
      ...defaultConfig().settings,
      nodeManager: manager,
      nvmDir: dir,
      fnmDir: dir,
    } as Settings;
    const panel = mount(settings);
    try {
      panel.submit();
      panel.export();
      expect(panel.save).not.toHaveBeenCalled();
      expect(panel.exportSettings).not.toHaveBeenCalled();
      expect(panel.error).toHaveBeenCalledTimes(2);
    } finally {
      panel.dispose();
    }
  });
  it.each(['C:\\example\\fnm', '\\\\example-server\\share\\fnm'])(
    '接受 Windows 绝对数据目录 %s',
    (dir) => {
      const panel = mount({ ...defaultConfig().settings, nodeManager: 'fnm', fnmDir: dir });
      try {
        panel.submit();
        expect(panel.save).toHaveBeenCalledWith(expect.objectContaining({ fnmDir: dir }));
      } finally {
        panel.dispose();
      }
    },
  );
  it.each(['nvm', 'fnm'] as const)(
    '检测 %s 唯一数据根，兼容旧 nvm DTO，并忽略其它管理器',
    async (manager) => {
      const local = version(manager, `/example/${manager}`);
      if (manager === 'nvm') delete local.manager;
      vi.mocked(api.nodeVersions).mockResolvedValue([
        local,
        { ...local, version: 'v24.0.0' },
        version(manager === 'nvm' ? 'fnm' : 'nvm', '/example/other'),
      ]);
      const panel = mount({ ...defaultConfig().settings, nodeManager: manager });
      try {
        panel.detect();
        await flush();
        expect(api.nodeVersions).toHaveBeenCalledWith({ manager });
        expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
          `/example/${manager}`,
        );
        expect(panel.state.settings[manager === 'fnm' ? 'fnmDir' : 'nvmDir'] || '').toBe('');
        panel.submit();
        expect(panel.save).toHaveBeenCalledWith(
          expect.objectContaining({
            [manager === 'fnm' ? 'fnmDir' : 'nvmDir']: `/example/${manager}`,
          }),
        );
      } finally {
        panel.dispose();
      }
    },
  );
  it('多个根不自动选择，用户选择后才写入草稿', async () => {
    vi.mocked(api.nodeVersions).mockResolvedValue([
      version('fnm', '/example/one'),
      version('fnm', '/example/two'),
    ]);
    const panel = mount({
      ...defaultConfig().settings,
      nodeManager: 'fnm',
      fnmDir: '/example/manual',
    });
    try {
      panel.detect();
      await flush();
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
        '/example/manual',
      );
      expect(panel.root.textContent).toContain('检测到多个版本数据目录');
      expect(panel.root.querySelector<HTMLSelectElement>('#settings-node-root')!.value).toBe('');
      await change(panel.root, '#settings-node-root', '/example/two');
      panel.submit();
      expect(panel.save).toHaveBeenCalledWith(expect.objectContaining({ fnmDir: '/example/two' }));
    } finally {
      panel.dispose();
    }
  });
  it('无目录时说明手动选择或终端默认，并可通过文件夹选择器配置', async () => {
    const panel = mount({ ...defaultConfig().settings, nodeManager: 'nvm' });
    try {
      panel.detect();
      await flush();
      expect(panel.root.textContent).toContain(
        '未检测到本地版本数据目录，可手动选择目录，或使用终端默认模式',
      );
      vi.mocked(api.choosePath).mockResolvedValue('/example/selected');
      panel.root
        .querySelector<HTMLButtonElement>('[aria-label="选择公共 Node.js 版本数据目录"]')!
        .click();
      await flush();
      expect(api.choosePath).toHaveBeenCalledWith(true);
      panel.submit();
      expect(panel.save).toHaveBeenCalledWith(
        expect.objectContaining({ nvmDir: '/example/selected' }),
      );
    } finally {
      panel.dispose();
    }
  });
  it('切换模式后忽略旧异步响应，返回原模式也不会接受旧响应', async () => {
    let resolve!: (versions: NodeVersion[]) => void;
    vi.mocked(api.nodeVersions).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const panel = mount({ ...defaultConfig().settings, nodeManager: 'nvm' });
    try {
      panel.detect();
      await change(panel.root, '#settings-node-manager', 'fnm');
      await change(panel.root, rootSelector, '/example/fnm');
      await change(panel.root, '#settings-node-manager', 'nvm');
      resolve([version('nvm', '/example/stale')]);
      await flush();
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe('');
      expect(panel.root.textContent).not.toContain('已检测到唯一版本数据目录');
    } finally {
      panel.dispose();
    }
  });
  it('检测期间手动修改目录不会被响应覆盖', async () => {
    let resolve!: (versions: NodeVersion[]) => void;
    vi.mocked(api.nodeVersions).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const panel = mount({ ...defaultConfig().settings, nodeManager: 'fnm' });
    try {
      panel.detect();
      await change(panel.root, rootSelector, '/example/manual');
      resolve([version('fnm', '/example/detected')]);
      await flush();
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
        '/example/manual',
      );
    } finally {
      panel.dispose();
    }
  });
  it.each(['saving', 'busy'] as const)('%s 禁用公共设置、检测、保存与导出', async (flag) => {
    const panel = mount({
      ...defaultConfig().settings,
      nodeManager: 'fnm',
      fnmDir: '/example/fnm',
    });
    try {
      panel.state[flag] = true;
      await flush();
      expect(panel.root.querySelector<HTMLFieldSetElement>('.settings-node-fields')!.disabled).toBe(
        true,
      );
      // happy-dom 的 :disabled 不继承 fieldset，直接验证表单控件处于禁用字段组中。
      for (const selector of [
        '#settings-node-manager',
        rootSelector,
        '[aria-label="选择公共 Node.js 版本数据目录"]',
      ]) {
        expect(panel.root.querySelector(selector)!.closest('fieldset[disabled]')).not.toBeNull();
      }
      panel.detect();
      panel.submit();
      panel.export();
      expect(api.nodeVersions).not.toHaveBeenCalled();
      expect(panel.save).not.toHaveBeenCalled();
      expect(panel.exportSettings).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('主题外部更新保留 Node 草稿，父保存回执同步全部公共字段', async () => {
    const panel = mount();
    try {
      await change(panel.root, '#settings-node-manager', 'fnm');
      await change(panel.root, rootSelector, '/example/draft');
      panel.state.settings = { ...panel.state.settings, theme: 'blue' };
      await flush();
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
        '/example/draft',
      );
      panel.submit();
      expect(panel.save).toHaveBeenCalledWith(
        expect.objectContaining({ theme: 'blue', fnmDir: '/example/draft' }),
      );
      panel.state.settings = {
        ...panel.state.settings,
        nodeManager: 'nvm',
        nvmDir: '/example/receipt',
        fnmDir: '/example/retained',
      };
      await flush();
      expect(panel.root.querySelector<HTMLSelectElement>('#settings-node-manager')!.value).toBe(
        'nvm',
      );
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
        '/example/receipt',
      );
      panel.submit();
      expect(panel.save).toHaveBeenLastCalledWith(
        expect.objectContaining({
          nodeManager: 'nvm',
          nvmDir: '/example/receipt',
          fnmDir: '/example/retained',
        }),
      );
      panel.state.settings = { ...panel.state.settings, nvmDir: '/example/new-root' };
      await flush();
      expect(panel.root.querySelector<HTMLInputElement>(rootSelector)!.value).toBe(
        '/example/new-root',
      );
    } finally {
      panel.dispose();
    }
  });
});
