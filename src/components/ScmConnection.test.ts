// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, expect, it, vi } from 'vitest';
import ScmConnection from './ScmConnection.vue';
import { api } from '../api';
const env = vi.hoisted(() => ({ desktop: true }));
vi.mock('../api', () => ({
  get desktop() {
    return env.desktop;
  },
  api: { scmStatus: vi.fn(), scmLogin: vi.fn(), scmLogout: vi.fn() },
}));
const status = {
  connected: true,
  baseUrl: 'https://example.invalid',
  username: 'example',
  remembered: false,
  message: null,
};
async function flush() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    await nextTick();
  }
}
function mount() {
  const root = document.createElement('div');
  const changed = vi.fn();
  const app = createApp({ render: () => h(ScmConnection, { onChange: changed }) });
  app.mount(root);
  return { root, changed, close: () => app.unmount() };
}
beforeEach(() => {
  vi.clearAllMocks();
  env.desktop = true;
  vi.mocked(api.scmStatus).mockResolvedValue({ ...status, connected: false });
  vi.mocked(api.scmLogin).mockResolvedValue(status);
});
it('恢复有效会话后仅显示地址、用户名及状态操作，不显示登录表单', async () => {
  vi.mocked(api.scmStatus).mockResolvedValueOnce(status);
  const panel = mount();
  try {
    await flush();
    expect(panel.root.querySelector('form')).toBeNull();
    expect(panel.root.querySelector('input')).toBeNull();
    expect(panel.root.querySelector('.connection-summary')?.textContent).toContain(status.baseUrl);
    expect(panel.root.querySelector('.connection-summary')?.textContent).toContain(status.username);
    expect(
      [...panel.root.querySelectorAll('button')].map((button) => button.textContent?.trim()),
    ).toEqual(['刷新状态', '退出登录']);
    expect(api.scmLogin).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('检查初始会话期间不闪现登录表单', async () => {
  let finish!: (value: typeof status) => void;
  vi.mocked(api.scmStatus).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const panel = mount();
  try {
    await nextTick();
    expect(panel.root.textContent).toContain('正在检查登录状态');
    expect(panel.root.querySelector('form')).toBeNull();
    finish(status);
    await flush();
    expect(panel.root.querySelector('.connection-summary')).not.toBeNull();
  } finally {
    panel.close();
  }
});
it('退出登录后恢复登录表单，保留地址和用户名但不保留密码', async () => {
  vi.mocked(api.scmStatus).mockResolvedValueOnce(status);
  vi.mocked(api.scmLogout).mockResolvedValueOnce({ ...status, connected: false });
  const panel = mount();
  try {
    await flush();
    [...panel.root.querySelectorAll('button')]
      .find((button) => button.textContent?.trim() === '退出登录')!
      .click();
    await flush();
    expect(api.scmLogout).toHaveBeenCalledOnce();
    expect(panel.root.querySelector('.connection-summary')).toBeNull();
    expect(panel.root.querySelector<HTMLInputElement>('[name="baseUrl"]')?.value).toBe(
      status.baseUrl,
    );
    expect(panel.root.querySelector<HTMLInputElement>('[name="username"]')?.value).toBe(
      status.username,
    );
    expect(panel.root.querySelector<HTMLInputElement>('[name="password"]')?.value).toBe('');
    expect(panel.root.querySelector('button[type="submit"]')?.textContent?.trim()).toBe('登录');
    expect(api.scmLogin).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('系统凭据失败的安全提示可见，刷新发现过期后清除已连接状态', async () => {
  vi.mocked(api.scmStatus).mockResolvedValueOnce({
    ...status,
    message: '系统凭据不可用，当前仅内存登录',
  });
  const panel = mount();
  try {
    await flush();
    expect(panel.root.textContent).toContain('系统凭据不可用');
    vi.mocked(api.scmStatus).mockRejectedValueOnce('SessionExpired: SCM 会话已过期');
    [...panel.root.querySelectorAll('button')]
      .find((button) => button.textContent?.trim() === '刷新状态')!
      .click();
    await flush();
    expect(panel.root.textContent).toContain('未连接');
    expect(panel.root.textContent).toContain('会话已过期');
    expect(panel.changed).toHaveBeenLastCalledWith(expect.objectContaining({ connected: false }));
    expect(panel.root.querySelector('.connection-summary')).toBeNull();
    expect(panel.root.querySelector('[name="password"]')).not.toBeNull();
  } finally {
    panel.close();
  }
});
it('登录成功清除密码且只发送安全状态，记住默认关闭', async () => {
  const panel = mount();
  await flush();
  for (const [name, value] of [
    ['baseUrl', 'https://example.invalid'],
    ['username', 'example'],
    ['password', 'secret'],
  ]) {
    const input = panel.root.querySelector(`[name="${name}"]`) as HTMLInputElement;
    input.value = value;
    input.dispatchEvent(new Event('input'));
  }
  panel.root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await flush();
  expect(api.scmLogin).toHaveBeenCalledWith('https://example.invalid', 'example', 'secret', false);
  expect((panel.root.querySelector('[name="password"]') as HTMLInputElement)?.value ?? '').toBe('');
  expect(JSON.stringify(panel.changed.mock.calls)).not.toContain('secret');
  panel.close();
});
it('浏览器预览不调用 native，卸载后忽略迟到状态', async () => {
  env.desktop = false;
  const preview = mount();
  await flush();
  expect(api.scmStatus).not.toHaveBeenCalled();
  preview.close();
  env.desktop = true;
  let finish!: (value: typeof status) => void;
  vi.mocked(api.scmStatus).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const panel = mount();
  panel.close();
  finish(status);
  await flush();
  expect(panel.changed).not.toHaveBeenCalled();
});
it('会话过期只提示重新登录，不自动发送凭据', async () => {
  vi.mocked(api.scmStatus).mockResolvedValueOnce({
    ...status,
    connected: false,
    message: 'sessionExpired',
  });
  const panel = mount();
  await flush();
  expect(panel.root.textContent).toContain('会话已过期，请重新登录');
  expect(api.scmLogin).not.toHaveBeenCalled();
  panel.close();
});
it('登录失败不展示密码且阻止重复登录', async () => {
  let fail!: (value: Error) => void;
  vi.mocked(api.scmLogin).mockReturnValue(
    new Promise((_, reject) => {
      fail = reject;
    }),
  );
  const panel = mount();
  await flush();
  for (const [name, value] of [
    ['baseUrl', 'https://example.invalid'],
    ['username', 'example'],
    ['password', 'secret'],
  ]) {
    const input = panel.root.querySelector(`[name="${name}"]`) as HTMLInputElement;
    input.value = value;
    input.dispatchEvent(new Event('input'));
  }
  const form = panel.root.querySelector('form')!;
  form.dispatchEvent(new Event('submit', { cancelable: true }));
  form.dispatchEvent(new Event('submit', { cancelable: true }));
  expect(api.scmLogin).toHaveBeenCalledTimes(1);
  fail(new Error('secret'));
  await flush();
  expect(panel.root.textContent).not.toContain('secret');
  panel.close();
});
