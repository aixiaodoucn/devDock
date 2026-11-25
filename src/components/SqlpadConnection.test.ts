// @vitest-environment happy-dom
import { createApp, nextTick } from 'vue';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import SqlpadConnection from './SqlpadConnection.vue';

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));
const offline = {
  connected: false,
  baseUrl: 'https://fixture.invalid',
  connections: [],
  selectedId: null,
};
const online = {
  ...offline,
  connected: true,
  connections: ['one', 'two'].map((id) => ({ id, name: `连接 ${id}`, driver: 'test' })),
};
const root = document.createElement('div');
let app: ReturnType<typeof createApp>;
async function settle() {
  await Promise.resolve();
  await nextTick();
}
async function mount() {
  document.body.append(root);
  app = createApp(SqlpadConnection);
  app.mount(root);
  await settle();
}
function fill(type: string, value: string) {
  const input = root.querySelector<HTMLInputElement>(`input[type=${type}]`)!;
  input.value = value;
  input.dispatchEvent(new Event('input'));
}
function submit() {
  root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
}
beforeEach(() => vi.resetAllMocks());
afterEach(() => {
  app.unmount();
  root.remove();
  vi.restoreAllMocks();
});

it('只向独立登录命令发送一次凭据，失败时清空表单并隐藏原始错误', async () => {
  const saved = vi.spyOn(Storage.prototype, 'setItem');
  vi.mocked(invoke).mockResolvedValue(offline);
  await mount();
  const email = 'test@example.invalid';
  const password = 'fixture-password-only';
  fill('email', email);
  fill('password', password);
  for (const url of [
    'http://fixture.invalid',
    'https://name:secret@fixture.invalid',
    'https://fixture.invalid/api',
    'https://fixture.invalid/?token=x',
  ]) {
    fill('url', url);
    submit();
    expect(invoke).toHaveBeenCalledTimes(1);
  }
  vi.mocked(invoke).mockRejectedValueOnce(new Error(`${email} ${password}`));
  fill('url', `${offline.baseUrl}/`);
  submit();
  await settle();
  expect(invoke).toHaveBeenLastCalledWith('sqlpad_login', {
    baseUrl: offline.baseUrl,
    email,
    password,
  });
  expect(root.textContent).toContain('登录失败');
  expect(root.textContent).not.toContain(password);
  expect(root.textContent).not.toContain(email);
  root.querySelector('button')!.click();
  await settle();
  for (const field of root.querySelectorAll<HTMLInputElement>(
    'input[type=email],input[type=password]',
  )) {
    expect(field.value).toBe('');
  }
  expect(saved).not.toHaveBeenCalled();
});

it('切换候选连接需要重新确认数据库权限，保存后采用后端状态，退出后移除连接', async () => {
  vi.mocked(invoke).mockResolvedValueOnce(online);
  await mount();
  expect(invoke).toHaveBeenCalledExactlyOnceWith('sqlpad_status', undefined);
  const select = root.querySelector('select')!;
  const save = root.querySelector<HTMLButtonElement>('.actions .primary')!;
  for (const choice of ['one', 'two']) {
    select.value = choice;
    select.dispatchEvent(new Event('change'));
    await nextTick();
    expect(save.disabled).toBe(true);
    root.querySelector<HTMLInputElement>('[type=checkbox]')!.click();
    await nextTick();
    expect(save.disabled).toBe(false);
  }
  vi.mocked(invoke).mockResolvedValueOnce({ ...online, selectedId: 'two' });
  save.click();
  await settle();
  expect(invoke).toHaveBeenLastCalledWith('sqlpad_select_connection', { connectionId: 'two' });
  expect(root.querySelector('.connection-summary dl > div:last-child dd')?.textContent).toBe(
    '连接 two',
  );
  vi.mocked(invoke).mockResolvedValueOnce(offline);
  root.querySelector<HTMLButtonElement>('.actions button:last-child')!.click();
  await settle();
  expect(invoke).toHaveBeenLastCalledWith('sqlpad_logout', undefined);
  expect(root.textContent).not.toContain('连接 two');
  expect(root.textContent).toContain('会话已失效');
  expect(root.textContent).toContain('主窗口逐次确认');
});

it('使用本次凭据独立登录，刷新发现会话失效后移除连接信息', async () => {
  vi.mocked(invoke)
    .mockResolvedValueOnce(offline)
    .mockResolvedValueOnce(online)
    .mockResolvedValueOnce(offline);
  await mount();
  fill('email', 'fixture@example.invalid');
  fill('password', 'test-only');
  submit();
  await settle();
  expect(root.querySelector('select')).toBeTruthy();
  expect(root.querySelector('[type=password]')).toBeNull();
  root.querySelector('header button')!.dispatchEvent(new Event('click'));
  await settle();
  expect(root.querySelector('select')).toBeNull();
  expect(root.textContent).toContain('会话已失效');
  expect(root.querySelector<HTMLInputElement>('[type=email]')?.value).toBe('');
  expect(root.querySelector<HTMLInputElement>('[type=password]')?.value).toBe('');
  expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
    'sqlpad_status',
    'sqlpad_login',
    'sqlpad_status',
  ]);
});
