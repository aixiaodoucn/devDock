// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { version } from '../../package.json';
import AppUpdater from './AppUpdater.vue';

const environment = vi.hoisted(() => ({ desktop: true }));
vi.mock('../api', () => ({
  get desktop() {
    return environment.desktop;
  },
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage = (_message: { downloaded: number; total: number | null }) => {};
  },
}));
const FOUR_HOURS = 4 * 60 * 60 * 1000;
const update = { version: '9.0.0', notes: '更新说明', downloaded: false };
const disposers: (() => void)[] = [];
async function flush() {
  await Promise.resolve();
  await nextTick();
}
function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
async function mount(initialBusy = false) {
  const busy = ref(initialBusy);
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({ render: () => h(AppUpdater, { busy: busy.value }) });
  app.mount(root);
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    app.unmount();
    root.remove();
  };
  disposers.push(dispose);
  await flush();
  return {
    root,
    busy,
    dispose,
    button(text: string) {
      const button = [...root.querySelectorAll('button')].find((item) =>
        item.textContent?.includes(text),
      );
      if (!button) throw new Error(`缺少按钮：${text}`);
      return button;
    },
  };
}
function calls(command: string) {
  return vi.mocked(invoke).mock.calls.filter(([name]) => name === command);
}
function progress() {
  const args = calls('download_update').at(-1)?.[1] as {
    onProgress: { onmessage: (value: { downloaded: number; total: number | null }) => void };
  };
  return args.onProgress;
}
describe('应用更新入口', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    environment.desktop = true;
    vi.mocked(invoke).mockResolvedValue(null);
  });
  afterEach(() => {
    disposers.splice(0).forEach((dispose) => dispose());
    vi.useRealTimers();
  });
  it('浏览器预览仅展示版本，不调用 IPC 或启动计时器', async () => {
    environment.desktop = false;
    const panel = await mount();
    expect(panel.root.textContent).toContain(version);
    panel.button(version).dispatchEvent(new MouseEvent('click'));
    await vi.advanceTimersByTimeAsync(FOUR_HOURS * 2);
    expect(invoke).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
  it('启动检查一次，每四小时检查，版本号可随时手动重试', async () => {
    const panel = await mount();
    expect(panel.root.textContent).toContain(version);
    expect(calls('check_update')).toHaveLength(1);
    expect(panel.root.textContent).not.toContain('重启更新');
    expect(panel.root.querySelectorAll('button')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(FOUR_HOURS - 1);
    expect(calls('check_update')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(calls('check_update')).toHaveLength(2);
    panel.button(version).click();
    await flush();
    expect(calls('check_update')).toHaveLength(3);
  });
  it('检查期间保留版本文字，仅更新悬浮提示，不插入会撑高布局的状态行', async () => {
    const pending = deferred<null>();
    vi.mocked(invoke).mockReturnValueOnce(pending.promise);
    const panel = await mount();
    expect(panel.button(version).title).toContain('正在检查更新');
    expect(panel.button(version).getAttribute('aria-busy')).toBe('true');
    expect(
      panel.root.querySelector('.app-updater-check-icon')?.classList.contains('is-checking'),
    ).toBe(true);
    expect(panel.root.querySelector('[role="status"]')).toBeNull();
    expect(panel.root.textContent?.trim()).toBe(`v${version}`);
    pending.resolve(null);
    await flush();
    expect(panel.root.querySelector('[role="status"]')).toBeNull();
    expect(panel.root.textContent?.trim()).toBe(`v${version}`);
    expect(panel.button(version).getAttribute('aria-busy')).toBe('false');
    expect(
      panel.root.querySelector('.app-updater-check-icon')?.classList.contains('is-checking'),
    ).toBe(false);
    const manual = deferred<null>();
    vi.mocked(invoke).mockReturnValueOnce(manual.promise);
    panel.button(version).click();
    await flush();
    expect(
      panel.root.querySelector('.app-updater-check-icon')?.classList.contains('is-checking'),
    ).toBe(true);
    manual.reject(new Error('检查失败'));
    await flush();
    expect(
      panel.root.querySelector('.app-updater-check-icon')?.classList.contains('is-checking'),
    ).toBe(false);
  });
  it('检查失败安静显示可重试 tooltip，检查未结束时避免重复调用', async () => {
    const pending = deferred<null>();
    vi.mocked(invoke).mockReturnValueOnce(pending.promise);
    const panel = await mount();
    panel.button(version).click();
    await vi.advanceTimersByTimeAsync(FOUR_HOURS);
    expect(calls('check_update')).toHaveLength(1);
    pending.reject(new Error('network offline'));
    await flush();
    expect(panel.button(version).title).toMatch(/检查失败.*重试/);
    expect(panel.root.querySelector('[role="alert"]')).toBeNull();
    panel.button(version).click();
    await flush();
    expect(calls('check_update')).toHaveLength(2);
  });
  it('下载进度不代表校验成功，下载和安装均防双击，暂停定时检查', async () => {
    const download = deferred();
    const install = deferred();
    vi.mocked(invoke)
      .mockResolvedValueOnce(update)
      .mockReturnValueOnce(download.promise)
      .mockReturnValueOnce(install.promise);
    const panel = await mount();
    const downloadButton = panel.button('更新');
    downloadButton.click();
    downloadButton.click();
    await flush();
    expect(calls('download_update')).toHaveLength(1);
    expect(progress()).toBeDefined();
    progress().onmessage({ downloaded: 100, total: 100 });
    await flush();
    expect(panel.root.textContent).not.toContain('重启更新');
    expect(panel.root.textContent).toContain('校验');
    panel.button(version).click();
    await vi.advanceTimersByTimeAsync(FOUR_HOURS * 2);
    expect(calls('check_update')).toHaveLength(1);
    download.resolve();
    await flush();
    expect(panel.root.textContent).toContain('下载完成');
    panel.button(version).click();
    await vi.advanceTimersByTimeAsync(FOUR_HOURS);
    expect(calls('check_update')).toHaveLength(1);
    const installButton = panel.button('重启更新');
    installButton.click();
    installButton.click();
    await flush();
    expect(calls('install_update')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(FOUR_HOURS);
    expect(calls('check_update')).toHaveLength(1);
    install.resolve();
  });
  it('未知或零 total 展示有限字节数，不显示 NaN/Infinity', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(update).mockReturnValueOnce(deferred().promise);
    const panel = await mount();
    panel.button('更新').click();
    for (const total of [null, 0]) {
      progress().onmessage({ downloaded: 1048576, total });
      await flush();
      expect(panel.root.textContent).toContain('1.0 MB');
      expect(panel.root.textContent).not.toMatch(/NaN|Infinity/);
    }
  });
  it('签名校验失败保留中文错误和重试下载入口，不进入可安装状态', async () => {
    const download = deferred();
    vi.mocked(invoke).mockResolvedValueOnce(update).mockReturnValueOnce(download.promise);
    const panel = await mount();
    panel.button('更新').click();
    progress().onmessage({ downloaded: 100, total: 100 });
    download.reject('signature verification failed');
    await flush();
    expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('下载失败');
    expect(panel.root.textContent).not.toContain('重启更新');
    panel.button('重试下载').click();
    await flush();
    expect(calls('download_update')).toHaveLength(2);
  });
  it('运行任务或保存时允许下载，禁止重启，空闲后恢复', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(update).mockResolvedValueOnce(undefined);
    const panel = await mount(true);
    expect(panel.button('更新').disabled).toBe(false);
    panel.button('更新').click();
    await flush();
    expect(panel.button('重启更新').disabled).toBe(true);
    expect(panel.button('重启更新').title).toMatch(/任务|保存/);
    panel.button('重启更新').dispatchEvent(new MouseEvent('click'));
    expect(calls('install_update')).toHaveLength(0);
    panel.busy.value = false;
    await flush();
    panel.button('重启更新').click();
    expect(calls('install_update')).toHaveLength(1);
  });
  it('Rust 拒绝安装时显示中文错误并允许重试，保留已下载状态', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ ...update, downloaded: true })
      .mockRejectedValueOnce('存在运行中的任务');
    const panel = await mount();
    panel.button('重启更新').click();
    await flush();
    expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('安装失败');
    expect(panel.root.textContent).toContain('存在运行中的任务');
    panel.button('重启更新').click();
    expect(calls('install_update')).toHaveLength(2);
    expect(calls('download_update')).toHaveLength(0);
  });
  it('下载失败后恢复四小时检查，重新检查仍发现更新时可继续下载', async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(update)
      .mockRejectedValueOnce('网络中断')
      .mockResolvedValueOnce(update);
    const panel = await mount();
    panel.button('更新').click();
    await flush();
    expect(panel.root.textContent).toContain('下载失败');
    await vi.advanceTimersByTimeAsync(FOUR_HOURS);
    expect(calls('check_update')).toHaveLength(2);
    expect(panel.button('重试下载').disabled).toBe(false);
  });
  it('销毁后忽略安装失败的延迟返回', async () => {
    const install = deferred();
    vi.mocked(invoke)
      .mockResolvedValueOnce({ ...update, downloaded: true })
      .mockReturnValueOnce(install.promise);
    const panel = await mount();
    panel.button('重启更新').click();
    panel.dispose();
    install.reject('安装中断');
    await flush();
    expect(panel.root.textContent).toBe('');
    expect(vi.getTimerCount()).toBe(0);
  });
  it('销毁后取消定时检查并忽略检查返回', async () => {
    const check = deferred<typeof update>();
    vi.mocked(invoke).mockReturnValueOnce(check.promise);
    const panel = await mount();
    panel.dispose();
    check.resolve(update);
    await flush();
    await vi.advanceTimersByTimeAsync(FOUR_HOURS * 2);
    expect(calls('check_update')).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });
  it('销毁后进度回调与下载返回不会更新 DOM 或遗留计时器', async () => {
    const download = deferred();
    vi.mocked(invoke).mockResolvedValueOnce(update).mockReturnValueOnce(download.promise);
    const panel = await mount();
    panel.button('更新').click();
    const channel = progress();
    panel.dispose();
    channel.onmessage({ downloaded: 10, total: null });
    download.resolve();
    await flush();
    expect(panel.root.textContent).toBe('');
    expect(vi.getTimerCount()).toBe(0);
  });
});
