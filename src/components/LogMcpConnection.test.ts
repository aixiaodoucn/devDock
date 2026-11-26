// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import ScmLogOptions from './ScmLogOptions.vue';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from '../api';
import LogMcpConnection from './LogMcpConnection.vue';

const env = vi.hoisted(() => ({ desktop: true }));
vi.mock('../api', () => ({
  get desktop() {
    return env.desktop;
  },
  api: {
    logMcpStatus: vi.fn(),
    logMcpRawStatus: vi.fn(async () => false),
    logMcpSetRaw: vi.fn(async (enabled: boolean) => enabled),
    logMcpMatchLimit: vi.fn(async () => 500),
    logMcpSetMatchLimit: vi.fn(async (limit: number) => limit),
    logMcpScanScope: vi.fn(),
    logMcpSetScanScope: vi.fn(),
    logMcpEnable: vi.fn(),
    logMcpDisable: vi.fn(),
    logMcpToken: vi.fn(),
    mcpClientStatus: vi.fn(),
    mcpClientConnect: vi.fn(),
    mcpClientDisconnect: vi.fn(),
    mcpClientTemplate: vi.fn(),
  },
}));
const stopped = { enabled: false, endpoint: '' };
const running = { enabled: true, endpoint: 'http://127.0.0.1:49152/mcp' };
const token = 'fixture-token-only';
const root = document.createElement('div');
let app: ReturnType<typeof createApp>;
const connected = ref(false);
const revision = ref(0);
async function refreshLogin(isConnected = connected.value) {
  connected.value = isConnected;
  revision.value++;
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
async function mount(isConnected = true) {
  connected.value = isConnected;
  app = createApp({
    render: () =>
      h('div', [
        env.desktop && h(ScmLogOptions, { connected: connected.value, key: revision.value }),
        h(LogMcpConnection),
      ]),
  });
  app.mount(root);
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
async function click(label: string) {
  const button = Array.from(root.querySelectorAll('button')).find(
    (b) => b.textContent?.trim() === label,
  );
  expect(button?.disabled).toBe(false);
  button!.click();
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
beforeEach(() => {
  vi.resetAllMocks();
  env.desktop = true;
  vi.mocked(api.mcpClientStatus).mockImplementation(async (client) => ({
    state: client === 'claude' ? 'available' : 'configured',
    message: `${client} 状态说明`,
  }));
  vi.mocked(api.logMcpScanScope).mockResolvedValue('first');
  vi.mocked(api.logMcpStatus).mockResolvedValue(stopped);
  vi.mocked(api.logMcpEnable).mockResolvedValue(running);
  vi.mocked(api.logMcpDisable).mockResolvedValue(stopped);
  vi.mocked(api.logMcpToken).mockResolvedValue(token);
});
afterEach(() => {
  app.unmount();
});

it('授权说明可见；启停使用本次地址，隐藏时从 DOM 移除令牌', async () => {
  await mount();
  expect(root.textContent).toMatch(/所有已保存映射的后端仓库/);
  await click('启用');
  expect(root.textContent).toContain(running.endpoint);
  expect(root.innerHTML).not.toContain(token);
  await click('显示令牌');
  expect(root.textContent).toContain(token);
  await click('隐藏令牌');
  expect(root.innerHTML).not.toContain(token);
  await click('停用');
  expect(root.innerHTML).not.toContain(running.endpoint);
  const endpoint = 'http://127.0.0.1:49153/mcp';
  vi.mocked(api.logMcpEnable).mockResolvedValue({ enabled: true, endpoint });
  vi.mocked(api.logMcpToken).mockResolvedValue('next-fixture-token');
  await click('启用');
  await click('显示令牌');
  expect(root.textContent).toContain(endpoint);
  expect(root.textContent).toContain('next-fixture-token');
  expect(root.innerHTML).not.toContain(token);
});
it('恢复运行状态只读取地址，令牌读取失败时隐藏原始错误且允许重试', async () => {
  vi.mocked(api.logMcpStatus).mockResolvedValue(running);
  vi.mocked(api.logMcpToken).mockRejectedValueOnce(new Error(token));
  await mount();
  expect(api.logMcpToken).not.toHaveBeenCalled();
  await click('显示令牌');
  expect(root.querySelector('[role="alert"]')).toBeTruthy();
  expect(root.innerHTML).not.toContain(token);
  await click('显示令牌');
  expect(root.textContent).toContain(token);
});
it('独立查询和刷新服务与注册状态，按客户端注册或断开', async () => {
  vi.mocked(api.mcpClientConnect).mockResolvedValue({ state: 'configured', message: '注册完成' });
  vi.mocked(api.mcpClientDisconnect).mockResolvedValue({ state: 'available', message: '断开完成' });
  await mount();
  expect(root.textContent).toContain('将桥接凭据保存在本机，供 stdio 客户端读取');
  expect(root.textContent).toContain('已停用');
  expect(root.textContent).toContain('codex 状态说明');
  await click('注册 Claude Code');
  expect(api.mcpClientConnect).toHaveBeenCalledExactlyOnceWith('claude');
  expect(root.textContent).toContain('注册完成');
  await click('断开 Codex CLI');
  expect(api.mcpClientDisconnect).toHaveBeenCalledExactlyOnceWith('codex');
  expect(api.logMcpEnable).not.toHaveBeenCalled();
  vi.mocked(api.mcpClientStatus).mockResolvedValue({ state: 'conflict', message: '已有同名配置' });
  await click('刷新状态');
  expect(api.logMcpStatus).toHaveBeenCalledTimes(2);
  expect(api.mcpClientStatus).toHaveBeenCalledTimes(4);
  expect(root.textContent).toContain('已有同名配置');
  expect(root.textContent).not.toMatch(/注册 Claude Code|断开 Codex CLI/);
});
it('展示并原样复制后端返回的 stdio 配置', async () => {
  const template = '{\n  "mcpServers": {}\n}';
  vi.mocked(api.mcpClientTemplate).mockResolvedValue(template);
  const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
  try {
    await mount();
    await click('查看 JSON 配置');
    expect(root.querySelector('pre')?.textContent).toBe(template);
    await click('复制 JSON');
    expect(copy).toHaveBeenCalledExactlyOnceWith(template);
  } finally {
    copy.mockRestore();
  }
});
it('读取匹配行数上限，仅允许保存范围内的整数并显示后端确认值', async () => {
  await mount();
  const input = root.querySelector<HTMLInputElement>('#log-mcp-match-limit')!;
  const save = Array.from(root.querySelectorAll('button')).find(
    (b) => b.textContent?.trim() === '保存上限',
  )!;
  expect(input.value).toBe('500');
  expect(api.logMcpMatchLimit).toHaveBeenCalled();
  for (const value of ['', '0', '10001', '1.5', '1', '10000']) {
    input.value = value;
    input.dispatchEvent(new Event('input'));
    await nextTick();
    expect(save.disabled).toBe(!['1', '10000'].includes(value));
  }
  vi.mocked(api.logMcpSetMatchLimit).mockResolvedValueOnce(9999);
  await click('保存上限');
  expect(api.logMcpSetMatchLimit).toHaveBeenCalledExactlyOnceWith(10000);
  expect(input.value).toBe('9999');
});

it('切换范围时等待后端确认后再勾选', async () => {
  let confirm!: (scope: 'all') => void;
  vi.mocked(api.logMcpSetScanScope).mockReturnValueOnce(
    new Promise((resolve) => (confirm = resolve)),
  );
  await mount();
  const first = root.querySelector<HTMLInputElement>('input[value="first"]')!;
  const all = root.querySelector<HTMLInputElement>('input[value="all"]')!;
  expect(first.checked).toBe(true);
  all.click();
  await nextTick();
  expect(api.logMcpSetScanScope).toHaveBeenCalledExactlyOnceWith('all');
  expect(root.querySelector('input:checked')).toBeNull();
  confirm('all');
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  expect(all.checked).toBe(true);
});

it('读取或更新失败均显示未知范围，刷新后恢复实际值', async () => {
  vi.mocked(api.logMcpScanScope).mockRejectedValue(new Error());
  vi.mocked(api.logMcpSetScanScope).mockRejectedValueOnce(new Error());
  await mount();
  expect(root.textContent).toContain('读取日志查询范围失败');
  vi.mocked(api.logMcpScanScope).mockResolvedValue('all');
  expect(root.querySelector('input:checked')).toBeNull();
  await refreshLogin();
  expect(root.querySelector<HTMLInputElement>('input[value="all"]')!.checked).toBe(true);
  root.querySelector<HTMLInputElement>('input[value="first"]')!.click();
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  expect(root.textContent).toContain('更新日志查询范围失败');
  expect(root.querySelector('input:checked')).toBeNull();
  await refreshLogin();
  expect(root.querySelector<HTMLInputElement>('input[value="all"]')!.checked).toBe(true);
  expect(root.querySelector('[role="alert"]')).toBeNull();
});

const rawCheckbox = () =>
  root.querySelector<HTMLInputElement>('input[type="checkbox"][aria-label="允许返回日志原文"]')!;

it('未登录时提示登录，登录后可在服务停用时确认或撤销原文授权', async () => {
  await mount(false);
  expect(api.logMcpRawStatus).not.toHaveBeenCalled();
  expect(root.textContent).toContain('请先登录日志 SCM');
  expect(rawCheckbox().disabled).toBe(true);
  await refreshLogin(true);
  expect(rawCheckbox().disabled).toBe(false);
  rawCheckbox().click();
  await nextTick();
  const confirmation = root.querySelector('dialog[open]')!;
  expect(confirmation.textContent).toMatch(/匹配行.*模型服务.*历史记录/);
  expect(confirmation.textContent).toMatch(/记住.*授权/);
  expect(root.textContent).toContain('实例范围跨重启保留');
  await click('取消');
  expect(rawCheckbox().checked).toBe(false);
  expect(api.logMcpSetRaw).not.toHaveBeenCalled();
  rawCheckbox().click();
  await nextTick();
  await click('确认开启');
  expect(api.logMcpSetRaw).toHaveBeenCalledWith(true);
  expect(rawCheckbox().checked).toBe(true);
  rawCheckbox().click();
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  expect(api.logMcpSetRaw).toHaveBeenLastCalledWith(false);
  expect(api.logMcpEnable).not.toHaveBeenCalled();
  expect(rawCheckbox().checked).toBe(false);
});

it('刷新登录状态恢复偏好，服务启停不重新读取，退出登录清除授权显示', async () => {
  vi.mocked(api.logMcpStatus).mockResolvedValue(running);
  vi.mocked(api.logMcpRawStatus).mockResolvedValue(true);
  await mount();
  expect(rawCheckbox().checked).toBe(true);
  vi.mocked(api.logMcpSetRaw).mockRejectedValueOnce(new Error('fixture'));
  rawCheckbox().click();
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  expect(rawCheckbox().disabled).toBe(true);
  expect(rawCheckbox().checked).toBe(false);
  vi.mocked(api.logMcpRawStatus).mockRejectedValueOnce(new Error('fixture'));
  await refreshLogin();
  expect(root.textContent).toContain('原文状态未知');
  await refreshLogin();
  expect(rawCheckbox().checked).toBe(true);
  const reads = vi.mocked(api.logMcpRawStatus).mock.calls.length;
  await click('停用');
  expect(rawCheckbox().disabled).toBe(false);
  expect(rawCheckbox().checked).toBe(true);
  await click('启用');
  expect(rawCheckbox().checked).toBe(true);
  expect(api.logMcpSetRaw).toHaveBeenCalledTimes(1);
  expect(api.logMcpRawStatus).toHaveBeenCalledTimes(reads);
  await refreshLogin(false);
  expect(rawCheckbox().disabled).toBe(true);
  expect(rawCheckbox().checked).toBe(false);
  vi.mocked(api.logMcpRawStatus).mockResolvedValue(false);
  await refreshLogin(true);
  expect(rawCheckbox().checked).toBe(false);
});

it('浏览器预览不读取原生状态或凭据', async () => {
  env.desktop = false;
  await mount();
  expect(api.logMcpStatus).not.toHaveBeenCalled();
  expect(api.logMcpToken).not.toHaveBeenCalled();
  expect(root.querySelector('button')).toBeNull();
});
