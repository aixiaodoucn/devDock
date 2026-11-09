// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import GitBranch from './GitBranch.vue';
import { api } from '../api';

vi.mock('../api', () => ({ desktop: true, api: { gitBranch: vi.fn() } }));
async function flush() {
  await Promise.resolve();
  await nextTick();
}
async function mount() {
  const path = ref('/tmp/first');
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({ render: () => h(GitBranch, { path: path.value }) });
  app.mount(root);
  await flush();
  return {
    path,
    root,
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
describe('项目当前 Git 分支', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.gitBranch).mockResolvedValue('feature/current');
  });
  it('只读展示项目分支，窗口重新获得焦点后刷新', async () => {
    const panel = await mount();
    try {
      expect(api.gitBranch).toHaveBeenCalledWith('/tmp/first');
      expect(panel.root.textContent).toContain('feature/current');
      vi.mocked(api.gitBranch).mockResolvedValue('main');
      window.dispatchEvent(new Event('focus'));
      await flush();
      expect(panel.root.textContent).toContain('main');
      expect(panel.root.querySelector('button')).toBeNull();
    } finally {
      panel.dispose();
    }
    const calls = vi.mocked(api.gitBranch).mock.calls.length;
    window.dispatchEvent(new Event('focus'));
    expect(api.gitBranch).toHaveBeenCalledTimes(calls);
  });
  it('切换项目时忽略前一个项目的延迟结果', async () => {
    let resolveFirst!: (branch: string) => void;
    vi.mocked(api.gitBranch).mockReturnValueOnce(
      new Promise<string>((resolve) => {
        resolveFirst = resolve;
      }),
    );
    const panel = await mount();
    try {
      panel.path.value = '/tmp/second';
      await flush();
      await flush();
      resolveFirst('old-project');
      await flush();
      expect(api.gitBranch).toHaveBeenLastCalledWith('/tmp/second');
      expect(panel.root.textContent).toContain('feature/current');
      expect(panel.root.textContent).not.toContain('old-project');
    } finally {
      panel.dispose();
    }
  });
  it('读取失败显示状态并保留错误原因，重新聚焦可恢复', async () => {
    vi.mocked(api.gitBranch).mockRejectedValueOnce(new Error('项目目录不存在'));
    const panel = await mount();
    try {
      expect(panel.root.textContent).toContain('分支读取失败');
      expect(panel.root.querySelector('[role="status"]')?.getAttribute('title')).toBe(
        '项目目录不存在',
      );
      window.dispatchEvent(new Event('focus'));
      await flush();
      expect(panel.root.textContent).toContain('feature/current');
    } finally {
      panel.dispose();
    }
  });
});
