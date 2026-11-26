// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import SettingsPage from './SettingsPage.vue';
import { api } from '../api';
import { defaultConfig, type NodeVersion, type Settings } from '../types';

vi.mock('../api', () => ({
  desktop: true,
  api: {
    choosePath: vi.fn(),
    nodeVersions: vi.fn(),
    scmStatus: vi.fn(),
    scmLogin: vi.fn(),
    scmLogout: vi.fn(),
    sqlpadExecutionHistory: vi.fn(),
    sqlpadExecutionDetail: vi.fn(),
    sqlpadHistoryLimit: vi.fn().mockResolvedValue(100),
    sqlpadSetHistoryLimit: vi.fn(),
    sqlpadClearHistory: vi.fn(),
    sqlpadDeleteExecution: vi.fn(),
    sqlpadRerunExecution: vi.fn(),
    sqlpadStatus: vi
      .fn()
      .mockResolvedValue({ connected: false, baseUrl: '', connections: [], selectedId: null }),
    logScmStatus: vi.fn(),
    logScmLogin: vi.fn(),
    logScmLogout: vi.fn(),
    mcpToolCatalog: vi.fn().mockResolvedValue([]),
    logMcpScanScope: vi.fn().mockResolvedValue('first'),
    logMcpRawStatus: vi.fn().mockResolvedValue(false),
    logMcpStatus: vi.fn(),
    logMcpToken: vi.fn(),
    mcpClientStatus: vi.fn(),
  },
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
beforeEach(() => vi.clearAllMocks());
describe('SettingsPage 公共 Node.js 环境', () => {
  beforeEach(() => {
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

it('重新执行期间阻止重复请求，结束后按桌面端顺序刷新并提示失败', async () => {
  const source = {
    id: 'source',
    startedAt: 1,
    tool: 'query',
    connection: 'demo',
    hasResult: false,
  };
  const replayable = { ...source, canRerun: true };
  const legacy = { ...source, id: 'legacy' };
  const blocked = { ...source, id: 'blocked', canRerun: false };
  const inserted = { ...replayable, id: 'new', startedAt: 3 };
  vi.mocked(api.sqlpadExecutionHistory)
    .mockResolvedValueOnce([legacy, replayable, blocked])
    .mockResolvedValueOnce([legacy, inserted, replayable, blocked])
    .mockResolvedValueOnce([inserted]);
  let finish!: () => void;
  vi.mocked(api.sqlpadRerunExecution)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    )
    .mockRejectedValueOnce(new Error('private endpoint'));
  const panel = mount();
  try {
    [...panel.root.querySelectorAll<HTMLButtonElement>('nav button')]
      .find((b) => b.textContent?.trim() === 'MCP 服务')!
      .click();
    await flush();
    panel.root.querySelector<HTMLButtonElement>('#mcp-history-tab')!.click();
    await flush();
    const history = panel.root.querySelector<HTMLElement>('#mcp-history-panel')!;
    const buttons = () => [
      ...history.querySelectorAll<HTMLButtonElement>('.history-row button.primary'),
    ];
    for (const button of [buttons()[0], buttons()[2]]) {
      expect(button.disabled).toBe(true);
      expect(
        document.getElementById(button.getAttribute('aria-describedby')!)?.textContent,
      ).toContain('安全重新执行');
      button.click();
    }
    const original = buttons()[1];
    expect(original.textContent?.trim()).toBe('重新执行');
    original.click();
    original.click();
    await flush();
    expect(api.sqlpadRerunExecution).toHaveBeenCalledTimes(1);
    expect(api.sqlpadRerunExecution).toHaveBeenCalledWith('source');
    expect(original.disabled).toBe(true);
    expect(original.textContent).toContain('重新执行中');
    expect(api.sqlpadExecutionHistory).toHaveBeenCalledTimes(1);
    finish();
    await flush();
    expect(buttons()).toHaveLength(4);
    expect(buttons()[2]).toBe(original);
    expect(original.disabled).toBe(false);
    original.click();
    await flush();
    expect(api.sqlpadExecutionHistory).toHaveBeenCalledTimes(3);
    expect(history.querySelector('[role=alert]')?.textContent).toContain('重新执行失败');
    expect(history.textContent).not.toContain('private endpoint');
    expect(buttons()).toHaveLength(1);
    expect(buttons()[0].disabled).toBe(false);
  } finally {
    panel.dispose();
  }
});

it('打开执行记录才加载 SQL 和结果，刷新可区分失败与空记录', async () => {
  const base = {
    id: 'one',
    startedAt: 1700000000000,
    tool: 'prod_db_query_readonly',
    connection: '只读连接',
    hasResult: true,
  };
  const sql = 'SELECT label FROM demo';
  vi.mocked(api.sqlpadExecutionDetail)
    .mockRejectedValueOnce(new Error('private endpoint'))
    .mockResolvedValueOnce({
      sql,
      result: { rows: [{ label: '<script>sample</script>' }] },
      error: null,
    })
    .mockResolvedValueOnce({ sql, result: null, error: '查询超时' });
  vi.mocked(api.sqlpadExecutionHistory)
    .mockResolvedValueOnce([base, { ...base, id: 'two', hasResult: false }])
    .mockRejectedValueOnce(new Error('private endpoint'))
    .mockResolvedValueOnce([]);
  const panel = mount();
  try {
    [...panel.root.querySelectorAll('nav button')]
      .find((button) => button.textContent?.trim() === 'MCP 服务')!
      .dispatchEvent(new Event('click'));
    await flush();
    expect(api.sqlpadExecutionHistory).not.toHaveBeenCalled();
    panel.root.querySelector<HTMLButtonElement>('#mcp-history-tab')!.click();
    await flush();
    const history = panel.root.querySelector<HTMLElement>('#mcp-history-panel')!;
    expect(api.sqlpadExecutionDetail).not.toHaveBeenCalled();
    expect(history.textContent).not.toContain(sql);
    expect(
      [...history.querySelectorAll('.record-status')].map((el) => el.textContent?.trim()),
    ).toEqual(['已返回结果', '未取得结果']);
    const rows = history.querySelectorAll('details');
    for (const row of rows) {
      row.open = true;
      row.dispatchEvent(new Event('toggle'));
      await flush();
      if (row === rows[0]) {
        expect(row.textContent).toContain('读取详情失败');
        expect(row.textContent).not.toContain('private endpoint');
        row.querySelector('button')!.click();
        await flush();
      }
    }
    expect(vi.mocked(api.sqlpadExecutionDetail).mock.calls).toEqual([['one'], ['one'], ['two']]);
    for (const text of [base.tool, base.connection, sql, '<script>sample</script>', '查询超时'])
      expect(history.textContent).toContain(text);
    expect(history.querySelector('script')).toBeNull();
    history.querySelector('button')!.click();
    await flush();
    expect(history.querySelector('[role=alert]')?.textContent).toContain('读取执行记录失败');
    expect(history.textContent).not.toContain('private endpoint');
    history.querySelector('button')!.click();
    await flush();
    expect(history.textContent).toContain('暂无执行记录');
    expect(history.textContent).not.toContain(sql);
  } finally {
    panel.dispose();
  }
});

it('校验并保存保留上限，删除和清空都需要再次确认', async () => {
  const first = { id: 'one', startedAt: 1, tool: 'query', connection: 'first', hasResult: true };
  const second = { ...first, id: 'two', connection: 'second' };
  vi.mocked(api.sqlpadExecutionHistory)
    .mockResolvedValueOnce([first, second])
    .mockResolvedValueOnce([first])
    .mockResolvedValueOnce([second])
    .mockResolvedValueOnce([]);
  vi.mocked(api.sqlpadHistoryLimit).mockRejectedValueOnce(new Error('private endpoint'));
  vi.mocked(api.sqlpadSetHistoryLimit)
    .mockRejectedValueOnce(new Error('private endpoint'))
    .mockResolvedValueOnce(1);
  vi.mocked(api.sqlpadDeleteExecution)
    .mockRejectedValueOnce(new Error('private endpoint'))
    .mockResolvedValueOnce();
  vi.mocked(api.sqlpadClearHistory)
    .mockRejectedValueOnce(new Error('private endpoint'))
    .mockResolvedValueOnce();
  const panel = mount();
  const button = (text: string) =>
    [...panel.root.querySelectorAll('button')].find((item) => item.textContent?.trim() === text)!;
  const click = async (text: string) => {
    button(text).click();
    await flush();
  };
  try {
    await click('MCP 服务');
    panel.root.querySelector<HTMLButtonElement>('#mcp-history-tab')!.click();
    await flush();
    const selector = 'input[aria-label="历史保留上限"]';
    const field = panel.root.querySelector<HTMLInputElement>(selector)!;
    expect(field.disabled).toBe(true);
    await click('重试');
    expect(field.value).toBe('100');
    for (const value of ['', '0', '1.5', '10001']) {
      await change(panel.root, selector, value);
      expect(button('保存上限').disabled).toBe(true);
    }
    await change(panel.root, selector, '10000');
    expect(button('保存上限').disabled).toBe(false);
    await change(panel.root, selector, '1');
    expect(panel.root.textContent).toContain('清理超出的最旧记录');
    await click('保存上限');
    expect(panel.root.textContent).toContain('保存保留上限失败');
    await click('保存上限');
    expect(api.sqlpadSetHistoryLimit).toHaveBeenCalledWith(1);
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(1);
    for (const [trigger, confirm, command, error] of [
      ['删除', '确认删除', api.sqlpadDeleteExecution, '删除执行记录失败'],
      ['清空记录', '确认清空', api.sqlpadClearHistory, '清空执行记录失败'],
    ] as const) {
      await click(trigger);
      await click('取消');
      expect(command).not.toHaveBeenCalled();
      await click(trigger);
      await click(confirm);
      expect(panel.root.textContent).toContain(error);
      await click(trigger);
      await click(confirm);
      expect(command).toHaveBeenCalledTimes(2);
    }
    expect(api.sqlpadDeleteExecution).toHaveBeenCalledWith('one');
    expect(panel.root.textContent).toContain('暂无执行记录');
    expect(panel.root.textContent).not.toContain('private endpoint');
    expect(button('清空记录').disabled).toBe(true);
  } finally {
    panel.dispose();
  }
});

it('偏好设置使用独立日志 SCM 凭据，登录和退出均清除旧令牌并刷新 MCP', async () => {
  const disconnected = {
    connected: false,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  };
  vi.mocked(api.logScmStatus).mockResolvedValue(disconnected);
  vi.mocked(api.logScmLogin).mockResolvedValue({
    ...disconnected,
    connected: true,
    username: 'logs',
  });
  vi.mocked(api.logScmLogout).mockResolvedValue(disconnected);
  vi.mocked(api.logMcpStatus).mockResolvedValue({
    enabled: true,
    endpoint: 'http://localhost/mcp',
  });
  vi.mocked(api.logMcpToken).mockResolvedValue('fixture-session-token');
  const panel = mount();
  try {
    await flush();
    expect(panel.root.querySelector('[aria-label="日志 SCM 登录"]')).toBeNull();
    const menu = [...panel.root.querySelectorAll<HTMLButtonElement>('nav button')];
    expect(menu.map((item) => item.textContent?.trim())).toEqual([
      '开发工具',
      '终端命令',
      '配置随你迁移',
      'MCP 服务',
      '外观主题',
    ]);
    expect(menu[0].getAttribute('aria-current')).toBe('page');
    menu[3].click();
    await flush();
    const tabs = [...panel.root.querySelectorAll('[role="tab"]')];
    expect(tabs.map((tab) => tab.textContent?.trim())).toEqual([
      '连接与权限',
      '工具目录',
      '执行记录',
    ]);
    expect(tabs[0].getAttribute('aria-selected')).toBe('true');
    expect(api.mcpToolCatalog).not.toHaveBeenCalled();
    expect(api.logMcpToken).not.toHaveBeenCalled();
    const login = panel.root.querySelector<HTMLElement>('[aria-label="日志 SCM 登录"]')!;
    expect(login).toBeTruthy();
    const rawCheckbox = () => login.querySelector<HTMLInputElement>('#log-raw-checkbox')!;
    expect(rawCheckbox().disabled).toBe(true);
    expect(api.logMcpRawStatus).not.toHaveBeenCalled();
    const cards = panel.root.querySelectorAll('#mcp-connections-panel section[aria-label]');
    expect([...cards].map((card) => card.getAttribute('aria-label'))).toEqual([
      '日志 SCM 登录',
      '生产 SQLPad 数据库',
      '日志 MCP',
    ]);
    expect(login.querySelector('input[value="all"]')).toBeTruthy();
    expect(
      login.querySelector('input[type="checkbox"][aria-label="允许返回日志原文"]'),
    ).toBeTruthy();
    expect(panel.root.querySelector('[aria-label="日志 MCP"] .query-options')).toBeNull();
    expect(panel.root.querySelector('[aria-label="生产 SQLPad 数据库"] form')).toBeTruthy();
    expect(api.sqlpadStatus).toHaveBeenCalledOnce();
    expect(api.logScmStatus).toHaveBeenCalled();
    await change(login, '[name=baseUrl]', 'https://example.invalid');
    await change(login, '[name=username]', 'logs');
    await change(login, '[name=password]', 'fixture');
    login.querySelector<HTMLInputElement>('[type=checkbox]')!.click();
    for (const action of ['login', 'logout']) {
      const tokenButton = Array.from(panel.root.querySelectorAll('button')).find(
        (b) => b.textContent?.trim() === '显示令牌',
      );
      // 换账号会重挂连接面板；等本机服务和原文授权状态均加载完再操作。
      await vi.waitFor(() => expect(tokenButton?.disabled).toBe(false));
      tokenButton!.click();
      await flush();
      expect(panel.root.textContent).toContain('fixture-session-token');
      vi.mocked(api.logMcpStatus).mockClear();
      if (action === 'login')
        login.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
      else
        Array.from(login.querySelectorAll('button'))
          .find((b) => b.textContent?.trim() === '退出登录')!
          .click();
      await flush();
      expect(panel.root.textContent).not.toContain('fixture-session-token');
      expect(api.logMcpStatus).toHaveBeenCalledOnce();
      expect(rawCheckbox().disabled).toBe(action === 'logout');
      expect(api.logMcpRawStatus).toHaveBeenCalledOnce();
    }
    expect(api.logScmLogin).toHaveBeenCalledExactlyOnceWith(
      'https://example.invalid',
      'logs',
      'fixture',
      true,
    );
    expect(api.logScmLogout).toHaveBeenCalledOnce();
    expect(api.scmStatus).not.toHaveBeenCalled();
    expect(api.scmLogin).not.toHaveBeenCalled();
    expect(api.scmLogout).not.toHaveBeenCalled();
  } finally {
    panel.dispose();
  }
});
