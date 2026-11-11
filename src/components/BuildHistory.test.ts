// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import BuildHistory from './BuildHistory.vue';
import type { BuildRecord } from '../types';
const mocks = vi.hoisted(() => ({
  desktop: true,
  history: vi.fn(),
  remove: vi.fn(),
  open: vi.fn(),
  listen: vi.fn(),
  settings: vi.fn(),
  saveSettings: vi.fn(),
}));
vi.mock('../api', () => ({
  get desktop() {
    return mocks.desktop;
  },
  api: {
    buildHistory: mocks.history,
    deleteBuildHistory: mocks.remove,
    openPath: mocks.open,
    buildHistorySettings: mocks.settings,
    saveBuildHistorySettings: mocks.saveSettings,
  },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
const disposers: (() => void)[] = [];
let handler: (event: { payload: string }) => void;
const unlisten = vi.fn();
async function flush() {
  await Promise.resolve();
  await nextTick();
  await Promise.resolve();
  await nextTick();
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function record(id = 'first', status: BuildRecord['status'] = 'success'): BuildRecord {
  return {
    id,
    projectId: 'one',
    projectName: '项目一',
    projectPath: '/one',
    mode: 'apk',
    versionName: '1.2.3',
    versionCode: '123',
    branch: 'main',
    startedAt: 1700000000000,
    finishedAt: 1700000065000,
    durationMs: 65000,
    status,
    summary: `摘要 ${id}`,
    artifacts: [
      { path: '/out/app.apk', exists: true },
      { path: '/out/missing.apk', exists: false },
    ],
  };
}
async function mount() {
  const projectId = ref('one');
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({ render: () => h(BuildHistory, { projectId: projectId.value }) });
  app.mount(root);
  let disposed = false;
  const dispose = () => {
    if (!disposed) {
      disposed = true;
      app.unmount();
      root.remove();
    }
  };
  disposers.push(dispose);
  await flush();
  return {
    root,
    projectId,
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
async function selectLimit(root: HTMLElement, limit: number) {
  const select = root.querySelector('select');
  expect(select).not.toBeNull();
  select!.value = String(limit);
  select!.dispatchEvent(new Event('change', { bubbles: true }));
  await flush();
}
const DISPLAY_LIMIT_KEY = 'dev-dock-history-display-limit';
const manyRecords = () => Array.from({ length: 65 }, (_, index) => record(`record-${index}`));
beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  mocks.desktop = true;
  mocks.history.mockResolvedValue([record()]);
  mocks.settings.mockResolvedValue({ retentionLimit: 100 });
  mocks.saveSettings.mockImplementation(async (retentionLimit: number) => ({
    retentionLimit,
    removedCount: 0,
  }));
  mocks.remove.mockResolvedValue(undefined);
  mocks.open.mockResolvedValue(undefined);
  mocks.listen.mockImplementation(async (_event, callback) => {
    handler = callback;
    return unlisten;
  });
});
afterEach(() => {
  disposers.splice(0).forEach((dispose) => dispose());
  vi.restoreAllMocks();
});
async function editRetention(panel: Awaited<ReturnType<typeof mount>>, value: string) {
  const input = panel.root.querySelector<HTMLInputElement>('input[aria-label="每项目保留上限"]');
  expect(input).not.toBeNull();
  input!.value = value;
  input!.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
}
describe('构建历史保留设置', () => {
  it('提高上限保存服务端返回值并刷新，展示偏好保持独立且支持1000条', async () => {
    mocks.history.mockResolvedValue(Array.from({ length: 1000 }, (_, i) => record(String(i))));
    mocks.saveSettings.mockResolvedValue({ retentionLimit: 500, removedCount: 3 });
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('100');
    expect(panel.root.textContent).toContain('所有项目');
    expect(panel.root.textContent).toContain('运行中');
    expect(panel.root.textContent).toContain('实际文件');
    await editRetention(panel, '1000');
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(1000);
    expect(panel.root.textContent).toContain('已移除 3 条');
    expect(mocks.history).toHaveBeenCalledTimes(2);
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('500');
    await selectLimit(panel.root, 1000);
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(1000);
    await selectLimit(panel.root, 10);
    expect(mocks.remove).not.toHaveBeenCalled();
    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
  });
  it('降低上限明确二次确认，取消不保存且重新打开复制已保存值', async () => {
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    await editRetention(panel, '10');
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('最旧');
    panel.button('取消').click();
    await flush();
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('100');
    await editRetention(panel, '20');
    panel.button('保存').click();
    await flush();
    panel.button('确认降低并保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(20);
  });
  it.each(['', '0', '-1', '1001', '1.5'])('非法上限 %j 不调用保存', async (value) => {
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    await editRetention(panel, value);
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('1 到 1000');
  });
  it('读取失败禁用保存，可重试加载真实策略', async () => {
    mocks.settings.mockRejectedValueOnce(new Error('设置不可读'));
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.textContent).toContain('设置不可读');
    expect(panel.button('保存').disabled).toBe(true);
    expect(panel.root.querySelector('input')?.value).toBe('');
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    mocks.settings.mockResolvedValue({ retentionLimit: 200 });
    panel.button('重试读取设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('200');
  });
  it('保存失败保留草稿与原策略，重试仍需降低确认', async () => {
    mocks.saveSettings.mockRejectedValueOnce(new Error('磁盘写失败'));
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    await editRetention(panel, '200');
    panel.button('保存').click();
    await flush();
    expect(panel.root.textContent).toContain('磁盘写失败');
    expect(panel.root.querySelector('input')?.value).toBe('200');
    await editRetention(panel, '50');
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
    panel.button('确认降低并保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenLastCalledWith(50);
  });
  it('全局空payload刷新设置和记录，其他项目事件不刷新', async () => {
    const panel = await mount();
    mocks.settings.mockResolvedValue({ retentionLimit: 500 });
    handler({ payload: 'other' });
    await flush();
    expect(mocks.settings).toHaveBeenCalledTimes(1);
    handler({ payload: '' });
    await flush();
    expect(mocks.settings).toHaveBeenCalledTimes(2);
    expect(mocks.history).toHaveBeenCalledTimes(2);
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('500');
  });
  it('保存中防重复、禁用删除与刷新，全局事件合并且切项目旧回调不串页', async () => {
    const pending = deferred<{ retentionLimit: number; removedCount: number }>();
    mocks.saveSettings.mockReturnValueOnce(pending.promise);
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    await editRetention(panel, '200');
    panel.button('详情').click();
    await flush();
    panel.button('保存').click();
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
    expect(panel.button('移除记录').disabled).toBe(true);
    expect(panel.button('刷新').disabled).toBe(true);
    handler({ payload: '' });
    await flush();
    expect(mocks.settings).toHaveBeenCalledTimes(1);
    expect(mocks.history).toHaveBeenCalledTimes(1);
    panel.projectId.value = 'two';
    mocks.history.mockResolvedValue([{ ...record('second'), projectId: 'two' }]);
    mocks.settings.mockResolvedValue({ retentionLimit: 200 });
    await flush();
    expect(panel.button('保留设置').disabled).toBe(true);
    pending.resolve({ retentionLimit: 200, removedCount: 5 });
    await flush();
    expect(panel.root.textContent).not.toContain('已移除 5 条');
    expect(mocks.history).toHaveBeenLastCalledWith('two');
    expect(mocks.settings).toHaveBeenCalledTimes(2);
    panel.button('详情').click();
    await flush();
    expect(panel.root.textContent).toContain('摘要 second');
  });
  it('删除与读取中禁止保存，延迟设置读取切项目后忽略旧值', async () => {
    const old = deferred<{ retentionLimit: number }>();
    mocks.settings.mockReturnValueOnce(old.promise);
    const panel = await mount();
    expect(panel.button('保留设置').disabled).toBe(true);
    panel.projectId.value = 'two';
    mocks.settings.mockResolvedValue({ retentionLimit: 300 });
    await flush();
    old.resolve({ retentionLimit: 10 });
    await flush();
    panel.button('保留设置').click();
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('300');
    await editRetention(panel, '500');
    panel.button('详情').click();
    await flush();
    panel.button('移除记录').click();
    await flush();
    const removing = deferred<void>();
    mocks.remove.mockReturnValueOnce(removing.promise);
    panel.button('确认移除').click();
    await flush();
    expect(panel.button('保存').disabled).toBe(true);
    panel.button('保存').click();
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    removing.resolve();
    await flush();
    const reading = deferred<BuildRecord[]>();
    mocks.history.mockReturnValueOnce(reading.promise);
    panel.button('刷新').click();
    await flush();
    expect(panel.button('保存').disabled).toBe(true);
    reading.resolve([]);
    await flush();
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).toHaveBeenCalledExactlyOnceWith(500);
  });
  it('全局设置更新保留正在编辑草稿，并按最新已保存值重新确认降低', async () => {
    const panel = await mount();
    panel.button('保留设置').click();
    await flush();
    await editRetention(panel, '200');
    mocks.settings.mockResolvedValue({ retentionLimit: 500 });
    handler({ payload: '' });
    await flush();
    expect(panel.root.querySelector('input')?.value).toBe('200');
    panel.button('保存').click();
    await flush();
    expect(mocks.saveSettings).not.toHaveBeenCalled();
    expect(panel.button('确认降低并保存')).toBeDefined();
  });
  it('浏览器禁用保留设置且不调用原生设置API', async () => {
    mocks.desktop = false;
    const panel = await mount();
    expect(panel.button('保留设置').disabled).toBe(true);
    expect(mocks.settings).not.toHaveBeenCalled();
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });
});
describe('构建历史', () => {
  it('默认最多显示20条并显示当前与总数，切换立即生效且不删除历史', async () => {
    mocks.history.mockResolvedValue(manyRecords());
    const panel = await mount();
    const select = panel.root.querySelector('select');
    expect(select?.getAttribute('aria-label')).toBe('最多显示');
    expect([...panel.root.querySelectorAll('option')].map((option) => option.value)).toEqual([
      '10',
      '20',
      '50',
      '100',
      '200',
      '500',
      '1000',
    ]);
    expect(select?.value).toBe('20');
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(20);
    expect(panel.root.textContent).toContain('显示 20 / 共 65 条');
    for (const limit of [10, 50, 100]) {
      await selectLimit(panel.root, limit);
      expect(panel.root.querySelectorAll('.history-row')).toHaveLength(Math.min(limit, 65));
      expect(panel.root.textContent).toContain(`显示 ${Math.min(limit, 65)} / 共 65 条`);
    }
    expect(mocks.history).toHaveBeenCalledOnce();
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it('展示偏好跨刷新、项目切换和重挂载保留', async () => {
    mocks.history.mockResolvedValue(manyRecords());
    const panel = await mount();
    await selectLimit(panel.root, 10);
    expect(localStorage.getItem(DISPLAY_LIMIT_KEY)).toBe('10');
    panel.button('刷新').click();
    await flush();
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(10);
    panel.projectId.value = 'two';
    await flush();
    expect(panel.root.querySelector('select')?.value).toBe('10');
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(10);
    panel.dispose();
    const remounted = await mount();
    expect(remounted.root.querySelector('select')?.value).toBe('10');
    expect(remounted.root.querySelectorAll('.history-row')).toHaveLength(10);
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it.each(['garbage', '0', '-10', '21', '20.5', 'null', ''])(
    '无效存储值 %j 安全回退20条',
    async (value) => {
      localStorage.setItem(DISPLAY_LIMIT_KEY, value);
      mocks.history.mockResolvedValue(manyRecords());
      const panel = await mount();
      expect(panel.root.querySelector('select')?.value).toBe('20');
      expect(panel.root.querySelectorAll('.history-row')).toHaveLength(20);
    },
  );
  it('存储读取异常回退20条，写入失败仍调整显示且不影响加载和移除', async () => {
    vi.spyOn(localStorage, 'getItem').mockImplementation(() => {
      throw new Error('storage blocked');
    });
    vi.spyOn(localStorage, 'setItem').mockImplementation(() => {
      throw new Error('storage blocked');
    });
    mocks.history.mockResolvedValue(manyRecords());
    const panel = await mount();
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(20);
    await selectLimit(panel.root, 10);
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(10);
    expect(panel.root.textContent).toContain('本机展示偏好未保存');
    expect(panel.root.textContent).toContain('本次设置已生效');
    expect(mocks.remove).not.toHaveBeenCalled();
    panel.button('刷新').click();
    await flush();
    expect(panel.root.querySelectorAll('.history-row')).toHaveLength(10);
    panel.button('详情').click();
    await flush();
    panel.button('移除记录').click();
    await flush();
    panel.button('确认移除').click();
    await flush();
    expect(mocks.remove).toHaveBeenCalledExactlyOnceWith('one', 'record-0');
    expect(panel.root.textContent).toContain('显示 10 / 共 64 条');
  });
  it('缩小上限清除隐藏条目的展开与确认状态', async () => {
    mocks.history.mockResolvedValue(manyRecords());
    const panel = await mount();
    panel.root.querySelectorAll<HTMLButtonElement>('.history-row')[15]!.click();
    await flush();
    panel.button('移除记录').click();
    await flush();
    expect(panel.root.querySelector('.history-confirm')).not.toBeNull();
    await selectLimit(panel.root, 10);
    await selectLimit(panel.root, 20);
    expect(panel.root.querySelector('.history-details')).toBeNull();
    panel.root.querySelectorAll<HTMLButtonElement>('.history-row')[15]!.click();
    await flush();
    expect(panel.root.querySelector('.history-confirm')).toBeNull();
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it('浏览器预览可保存展示偏好但不访问原生历史', async () => {
    mocks.desktop = false;
    const panel = await mount();
    await selectLimit(panel.root, 50);
    expect(localStorage.getItem(DISPLAY_LIMIT_KEY)).toBe('50');
    expect(panel.root.textContent).toContain('浏览器预览无法读取历史记录');
    expect(mocks.history).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
    expect(mocks.remove).not.toHaveBeenCalled();
  });
  it('显示版本、平台、分支、时间耗时及所有历史状态，空态可刷新', async () => {
    mocks.history.mockResolvedValue(
      ['running', 'success', 'error', 'cancelled', 'interrupted'].map((status) =>
        record(status, status as BuildRecord['status']),
      ),
    );
    const panel = await mount();
    for (const text of [
      '1.2.3',
      '123',
      'Android APK',
      'main',
      '1分5秒',
      '构建中',
      '成功',
      '失败',
      '已取消',
      '已中断',
    ])
      expect(panel.root.textContent).toContain(text);
    expect(panel.root.querySelector('time')).not.toBeNull();
    mocks.history.mockResolvedValue([]);
    panel.button('刷新').click();
    await flush();
    expect(panel.root.textContent).toContain('暂无构建历史');
  });
  it('展开摘要和产物，缺失文件禁用，仅已有文件可定位', async () => {
    const panel = await mount();
    panel.button('详情').click();
    await flush();
    expect(panel.root.textContent).toContain('摘要 first');
    expect(panel.root.textContent).toContain('/out/missing.apk');
    expect(panel.root.textContent).toContain('已缺失');
    const buttons = [...panel.root.querySelectorAll<HTMLButtonElement>('button')].filter((button) =>
      button.textContent?.includes('定位'),
    );
    expect(buttons).toHaveLength(2);
    expect(buttons[1]!.disabled).toBe(true);
    buttons[1]!.click();
    buttons[0]!.click();
    await flush();
    expect(mocks.open).toHaveBeenCalledExactlyOnceWith('/out/app.apk');
    expect(panel.root.textContent).not.toContain('完整日志');
  });
  it('移除必须二次确认且仅删除记录，running记录不可移除', async () => {
    const panel = await mount();
    panel.button('详情').click();
    await flush();
    panel.button('移除记录').click();
    await flush();
    expect(mocks.remove).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('不删除实际产物');
    panel.button('取消').click();
    await flush();
    expect(mocks.remove).not.toHaveBeenCalled();
    panel.button('移除记录').click();
    await flush();
    panel.button('确认移除').click();
    await flush();
    expect(mocks.remove).toHaveBeenCalledExactlyOnceWith('one', 'first');
    expect(mocks.open).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('暂无构建历史');
    mocks.history.mockResolvedValue([record('active', 'running')]);
    panel.button('刷新').click();
    await flush();
    panel.button('详情').click();
    await flush();
    expect(
      [...panel.root.querySelectorAll('button')].some((button) =>
        button.textContent?.includes('移除记录'),
      ),
    ).toBe(false);
  });
  it('加载失败可刷新重试，删除和定位失败保留错误原因', async () => {
    mocks.history.mockRejectedValueOnce(new Error('读取失败'));
    const panel = await mount();
    expect(panel.root.textContent).toContain('读取失败');
    panel.button('刷新').click();
    await flush();
    panel.button('详情').click();
    await flush();
    mocks.open.mockRejectedValueOnce(new Error('文件不可访问'));
    panel.button('定位').click();
    await flush();
    expect(panel.root.textContent).toContain('文件不可访问');
    panel.button('移除记录').click();
    await flush();
    mocks.remove.mockRejectedValueOnce(new Error('写入失败'));
    panel.button('确认移除').click();
    await flush();
    expect(panel.root.textContent).toContain('写入失败');
    expect(panel.root.textContent).toContain('1.2.3');
  });
  it('切项目时忽略旧读取和删除结果，不串项目', async () => {
    const old = deferred<BuildRecord[]>();
    mocks.history.mockReturnValueOnce(old.promise);
    const panel = await mount();
    panel.projectId.value = 'two';
    mocks.history.mockResolvedValue([{ ...record('second'), projectId: 'two' }]);
    await flush();
    old.resolve([record('old')]);
    await flush();
    panel.button('详情').click();
    await flush();
    expect(panel.root.textContent).toContain('摘要 second');
    expect(panel.root.textContent).not.toContain('摘要 old');
    const removing = deferred<void>();
    mocks.remove.mockReturnValue(removing.promise);
    panel.button('移除记录').click();
    await flush();
    panel.button('确认移除').click();
    panel.projectId.value = 'three';
    mocks.history.mockResolvedValue([{ ...record('third'), projectId: 'three' }]);
    await flush();
    removing.resolve();
    await flush();
    panel.button('详情').click();
    await flush();
    expect(panel.root.textContent).toContain('摘要 third');
  });
  it('删除中防重并阻止刷新覆盖，期间事件完成后补刷新', async () => {
    const panel = await mount();
    const pending = deferred<void>();
    mocks.remove.mockReturnValue(pending.promise);
    panel.button('详情').click();
    await flush();
    panel.button('移除记录').click();
    await flush();
    panel.button('确认移除').click();
    panel.button('确认移除').click();
    await flush();
    expect(mocks.remove).toHaveBeenCalledTimes(1);
    expect(panel.button('刷新').disabled).toBe(true);
    mocks.history.mockResolvedValue([]);
    handler({ payload: 'one' });
    await flush();
    expect(mocks.history).toHaveBeenCalledTimes(1);
    pending.resolve();
    await flush();
    expect(mocks.history).toHaveBeenCalledTimes(2);
  });
  it('仅响应当前项目事件，读取中事件合并刷新，卸载释放监听', async () => {
    const panel = await mount();
    expect(mocks.listen).toHaveBeenCalledWith('build-history-updated', expect.any(Function));
    handler({ payload: 'other' });
    await flush();
    expect(mocks.history).toHaveBeenCalledTimes(1);
    const pending = deferred<BuildRecord[]>();
    mocks.history.mockReturnValueOnce(pending.promise);
    handler({ payload: 'one' });
    await flush();
    handler({ payload: 'one' });
    pending.resolve([record()]);
    await flush();
    expect(mocks.history).toHaveBeenCalledTimes(3);
    panel.dispose();
    expect(unlisten).toHaveBeenCalledOnce();
    handler({ payload: 'one' });
    await flush();
    expect(mocks.history).toHaveBeenCalledTimes(3);
  });
  it('异步注册晚于卸载仍立即释放，浏览器不读历史或监听', async () => {
    const registration = deferred<() => void>();
    mocks.listen.mockReturnValueOnce(registration.promise);
    const panel = await mount();
    panel.dispose();
    registration.resolve(unlisten);
    await flush();
    expect(unlisten).toHaveBeenCalledOnce();
    mocks.desktop = false;
    mocks.history.mockClear();
    mocks.listen.mockClear();
    const preview = await mount();
    expect(preview.root.textContent).toContain('桌面应用');
    expect(mocks.history).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
  });
});
