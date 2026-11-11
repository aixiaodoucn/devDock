// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import EnvironmentCheck from './EnvironmentCheck.vue';
import { defaultConfig, newProject } from '../types';

const mocks = vi.hoisted(() => ({ desktop: true, check: vi.fn(), request: vi.fn() }));
vi.mock('../api', () => ({
  get desktop() {
    return mocks.desktop;
  },
  api: { checkEnvironment: mocks.check, requestAccessibilityPermission: mocks.request },
}));
const disposers: (() => void)[] = [];
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
const report = (projectId = 'one', detail = '需要辅助功能权限') => ({
  projectId,
  checkedAt: 1700000000000,
  items: [
    { id: 'path', label: '项目目录', status: 'pass', detail: '目录可读', action: null },
    {
      id: 'accessibility',
      label: '辅助功能',
      status: 'warning',
      detail,
      action: 'request-accessibility',
    },
    { id: 'jdk', label: 'JDK', status: 'error', detail: '路径不存在', action: 'settings' },
    { id: 'skip', label: '原生工程', status: 'skipped', detail: '当前项目无需检查', action: null },
  ],
});
async function mount() {
  const project = ref({ ...newProject(), id: 'one', name: '项目一' });
  const settings = ref(defaultConfig().settings);
  const close = vi.fn(),
    edit = vi.fn(),
    preferences = vi.fn();
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(EnvironmentCheck, {
        project: project.value,
        settings: settings.value,
        onClose: close,
        onEdit: edit,
        onSettings: preferences,
      }),
  });
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
    project,
    settings,
    close,
    edit,
    preferences,
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
beforeEach(() => {
  vi.resetAllMocks();
  mocks.desktop = true;
  mocks.check.mockResolvedValue(report());
  mocks.request.mockResolvedValue(true);
});
afterEach(() => disposers.splice(0).forEach((dispose) => dispose()));
describe('环境体检', () => {
  it('挂载执行只读检查并展示状态、统计与时间，不自动请求授权', async () => {
    const panel = await mount();
    expect(panel.root.querySelector('[role="dialog"]')?.getAttribute('aria-modal')).toBe('true');
    expect(mocks.check).toHaveBeenCalledTimes(1);
    for (const text of ['通过 1', '错误 1', '警告 1', '已跳过', '目录可读', '检查时间'])
      expect(panel.root.textContent).toContain(text);
    expect(mocks.request).not.toHaveBeenCalled();
  });
  it('仅点击才请求授权，独立防重，返回后提示重新检查而不宣称已授权', async () => {
    const pending = deferred<boolean>();
    mocks.request.mockReturnValue(pending.promise);
    const panel = await mount();
    panel.button('请求辅助功能权限').click();
    panel.button('请求辅助功能权限').click();
    await flush();
    expect(mocks.request).toHaveBeenCalledTimes(1);
    expect(panel.button('授权处理中').disabled).toBe(true);
    pending.resolve(true);
    await flush();
    expect(panel.root.textContent).toContain('重新体检');
    expect(panel.root.textContent).not.toContain('授权成功');
    expect(mocks.check).toHaveBeenCalledTimes(1);
    panel.button('重新体检').click();
    await flush();
    expect(mocks.check).toHaveBeenCalledTimes(2);
  });
  it('检查失败可重试，权限请求失败保留原因', async () => {
    mocks.check.mockRejectedValueOnce(new Error('目录不可访问'));
    const panel = await mount();
    expect(panel.root.textContent).toContain('目录不可访问');
    panel.button('重新体检').click();
    await flush();
    mocks.request.mockRejectedValueOnce(new Error('系统拒绝'));
    panel.button('请求辅助功能权限').click();
    await flush();
    expect(panel.root.textContent).toContain('系统拒绝');
    expect(panel.button('请求辅助功能权限').disabled).toBe(false);
  });
  it('切换项目或修改配置时忽略旧请求结果并允许新检查', async () => {
    const first = deferred<ReturnType<typeof report>>(),
      second = deferred<ReturnType<typeof report>>();
    mocks.check
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockResolvedValue(report('two', '最新设置'));
    const panel = await mount();
    panel.project.value.id = 'two';
    await flush();
    panel.settings.value.jdkPath = '/new-jdk';
    await flush();
    second.resolve(report('two', '旧配置'));
    first.resolve(report('one', '旧项目'));
    await flush();
    expect(mocks.check).toHaveBeenCalledTimes(3);
    expect(panel.root.textContent).toContain('最新设置');
    expect(panel.root.textContent).not.toContain('旧配置');
    expect(panel.root.textContent).not.toContain('旧项目');
  });
  it('检查中防止重复，关闭后忽略迟到结果，发出编辑和设置事件', async () => {
    const pending = deferred<ReturnType<typeof report>>();
    mocks.check.mockReturnValue(pending.promise);
    const panel = await mount();
    expect(panel.button('检查中').disabled).toBe(true);
    panel.button('编辑项目').click();
    panel.button('偏好设置').click();
    expect(panel.edit).toHaveBeenCalledOnce();
    expect(panel.preferences).toHaveBeenCalledOnce();
    panel.button('关闭').click();
    expect(panel.close).toHaveBeenCalledOnce();
    pending.resolve(report());
    await flush();
    expect(panel.root.textContent).not.toContain('目录可读');
  });
  it('切项目后忽略旧授权结果，已通过的权限不再提供授权按钮', async () => {
    const pending = deferred<boolean>();
    mocks.request.mockReturnValue(pending.promise);
    const panel = await mount();
    panel.button('请求辅助功能权限').click();
    const checked = report('two');
    checked.items[1]!.status = 'pass';
    mocks.check.mockResolvedValue(checked);
    panel.project.value.id = 'two';
    await flush();
    pending.resolve(true);
    await flush();
    expect(panel.root.textContent).not.toContain('请求辅助功能权限');
    expect(panel.root.textContent).not.toContain('授权请求已发送');
  });
  it('浏览器预览展示桌面说明且不发任何IPC', async () => {
    mocks.desktop = false;
    const panel = await mount();
    expect(panel.root.textContent).toContain('桌面应用');
    expect(mocks.check).not.toHaveBeenCalled();
    expect(mocks.request).not.toHaveBeenCalled();
    expect(panel.button('重新体检').disabled).toBe(true);
  });
});
