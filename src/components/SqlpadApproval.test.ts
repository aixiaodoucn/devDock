// @vitest-environment happy-dom
import { createApp, nextTick } from 'vue';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import SqlpadApproval from './SqlpadApproval.vue';

const main = vi.hoisted(() => ({ label: 'main', listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => main }));
const request = { requestId: 'a', connection: '生产只读', preview: '<schema>', expiresAt: 0 };
const stop = vi.fn();
const root = document.createElement('div');
let app: ReturnType<typeof createApp> | undefined;
let notify: () => Promise<void>;
async function settle() {
  for (let i = 0; i < 4; i++) await nextTick();
}
async function mount() {
  document.body.append(root);
  app = createApp(SqlpadApproval);
  app.mount(root);
  await settle();
}
beforeEach(() => {
  vi.resetAllMocks();
  main.label = 'main';
  main.listen.mockImplementation(async (_name, callback) => {
    notify = callback;
    return stop;
  });
  request.expiresAt = Date.now() + 60_000;
  vi.mocked(invoke).mockResolvedValue(request);
});
afterEach(() => {
  app?.unmount();
  root.remove();
  vi.useRealTimers();
});

it.each([false, true])(
  '只提交显式决定 approved=%s，绑定展示中的请求且不重复提交',
  async (approved) => {
    await mount();
    expect(main.listen).toHaveBeenCalledWith('sqlpad-approval-changed', expect.any(Function));
    expect(invoke).toHaveBeenCalledExactlyOnceWith('sqlpad_pending_approval', undefined);
    expect(root.textContent).toContain('返回的生产数据将发送给 AI');
    expect(root.textContent).toContain(request.connection);
    expect(root.querySelector('pre')?.textContent).toBe(request.preview);
    vi.mocked(invoke).mockClear().mockResolvedValue(null);
    const button = root.querySelectorAll('button')[approved ? 1 : 0];
    button.click();
    button.click();
    await settle();
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['sqlpad_decide_approval', { requestId: 'a', approved }],
      ['sqlpad_pending_approval', undefined],
    ]);
    expect(root.querySelector('dialog')).toBeNull();
  },
);
it('取消通知淘汰晚到的快照，重挂后重新读取真实请求', async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise((done) => (resolve = done)));
  await mount();
  vi.mocked(invoke).mockResolvedValueOnce(null);
  await notify();
  resolve(request);
  await settle();
  expect(root.querySelector('dialog')).toBeNull();
  app!.unmount();
  expect(stop).toHaveBeenCalledOnce();
  await mount();
  expect(root.querySelector('dialog')).not.toBeNull();
  expect(vi.mocked(invoke).mock.calls.every(([name]) => name === 'sqlpad_pending_approval')).toBe(
    true,
  );
});
it('到期后移除审批，不发送决定', async () => {
  vi.useFakeTimers();
  await mount();
  await vi.advanceTimersByTimeAsync(60_001);
  expect(root.querySelector('dialog')).toBeNull();
  expect(invoke).toHaveBeenCalledOnce();
});
it('监听失败不读取、不展示也不发送决定', async () => {
  main.listen.mockRejectedValueOnce(new Error('fixture'));
  await mount();
  expect(invoke).not.toHaveBeenCalled();
  expect(root.querySelector('dialog')).toBeNull();
});
it('监听注册完成前卸载会释放监听且不读取请求', async () => {
  let finish!: (stop: () => void) => void;
  main.listen.mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
  await mount();
  app!.unmount();
  app = undefined;
  finish(stop);
  await settle();
  expect(stop).toHaveBeenCalledOnce();
  expect(invoke).not.toHaveBeenCalled();
});
it('陈旧决定被拒后重新展示下一次审批，不沿用上次决定', async () => {
  await mount();
  vi.mocked(invoke)
    .mockClear()
    .mockRejectedValueOnce(new Error('stale'))
    .mockResolvedValueOnce({ ...request, requestId: 'b', preview: '新元数据请求' });
  root.querySelectorAll('button')[1].click();
  await settle();
  expect(root.querySelector('pre')?.textContent).toBe('新元数据请求');
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ['sqlpad_decide_approval', { requestId: 'a', approved: true }],
    ['sqlpad_pending_approval', undefined],
  ]);
});
it('非主窗口不订阅也不读取', async () => {
  main.label = 'other';
  await mount();
  expect(main.listen).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
});
