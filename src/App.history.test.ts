// @vitest-environment happy-dom
import { createApp, nextTick } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.vue';
import { api } from './api';
import { defaultConfig, newProject } from './types';

vi.mock('./api', () => ({ desktop: false, api: { load: vi.fn(), buildHistory: vi.fn() } }));
const disposers: (() => void)[] = [];
async function flush() {
  await Promise.resolve();
  await nextTick();
  await Promise.resolve();
  await nextTick();
}
async function mount() {
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp(App);
  app.mount(root);
  disposers.push(() => {
    app.unmount();
    root.remove();
  });
  await flush();
  return {
    root,
    async click(text: string) {
      const button = [...root.querySelectorAll('button')].find(
        (item) => item.textContent?.trim() === text,
      );
      if (!button) throw new Error(`缺少按钮：${text}`);
      button.click();
      await flush();
    },
  };
}

describe('构建历史独立页面', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    localStorage.clear();
    const project = {
      ...newProject(),
      id: 'first',
      name: '示例项目',
      path: '/example/first',
      type: 'uniapp' as const,
      platforms: ['app'],
    };
    const second = { ...project, id: 'second', name: '另一个项目', path: '/example/second' };
    vi.mocked(api.load).mockResolvedValue({ ...defaultConfig(), projects: [project, second] });
  });
  afterEach(() => disposers.splice(0).forEach((dispose) => dispose()));

  it('从构建配置标题进入历史页，返回时保留构建目标', async () => {
    const panel = await mount();
    expect(panel.root.querySelector('.build-panel .panel-heading button')?.textContent).toContain(
      '构建历史',
    );
    expect(panel.root.querySelector('.build-history')).toBeNull();
    panel.root.querySelectorAll<HTMLButtonElement>('.build-target')[2].click();
    await panel.click('构建历史');
    expect(panel.root.querySelector('.history-page')?.textContent).toContain('示例项目');
    expect(panel.root.querySelector('.build-panel')).toBeNull();
    expect(panel.root.querySelector('.breadcrumbs')?.textContent).toContain('构建历史');
    expect(api.buildHistory).not.toHaveBeenCalled();
    await panel.click('返回构建配置');
    expect(panel.root.querySelector('.history-page')).toBeNull();
    expect(panel.root.querySelector('.build-target.selected')?.textContent).toContain('iOS');
  });

  it('历史页切换侧栏项目时返回该项目详情', async () => {
    const panel = await mount();
    await panel.click('构建历史');
    const projectButton = [
      ...panel.root.querySelectorAll<HTMLButtonElement>('aside.sidebar button'),
    ].find((item) => item.textContent?.includes('另一个项目'));
    expect(projectButton).toBeDefined();
    projectButton!.click();
    await flush();
    expect(panel.root.querySelector('.history-page')).toBeNull();
    expect(panel.root.querySelector('.project-heading-line h1')?.textContent).toContain(
      '另一个项目',
    );
    expect(panel.root.querySelector('.build-panel')).not.toBeNull();
  });
});
