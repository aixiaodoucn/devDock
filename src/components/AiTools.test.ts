// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AiLauncher from './AiLauncher.vue';
import SettingsPage from './SettingsPage.vue';
import { defaultConfig, type Settings } from '../types';

vi.mock('../api', () => ({ desktop: true, api: { choosePath: vi.fn() } }));
const tools = [
  { id: 'claude', name: 'Claude Code', command: 'claude', enabled: true },
  { id: 'codex', name: 'Codex', command: 'codex --profile work', enabled: true },
  { id: 'hidden', name: 'Disabled', command: 'unused', enabled: false },
];
function render(component: Parameters<typeof createApp>[0]) {
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp(component);
  app.mount(root);
  return {
    root,
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
describe('项目 AI 工具', () => {
  beforeEach(() => localStorage.clear());
  it('只显示启用工具，将所选工具交给项目入口，配置变化后清理旧选择', async () => {
    const state = reactive({ tools: [...tools] });
    const launch = vi.fn();
    const configure = vi.fn();
    const panel = render({
      render: () =>
        h(AiLauncher, {
          tools: state.tools,
          busy: false,
          onLaunch: launch,
          onConfigure: configure,
        }),
    });
    try {
      expect(panel.root.querySelectorAll('option')).toHaveLength(3);
      const select = panel.root.querySelector('select')!;
      expect(select.value).toBe('');
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).not.toHaveBeenCalled();
      expect(configure).toHaveBeenCalledOnce();
      expect(panel.root.textContent).toContain('请选择 AI 工具');
      select.value = 'codex';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenCalledWith(tools[1]);
      expect(localStorage.getItem('dev-dock-preferred-ai-tool')).toBe('codex');
      panel.root.querySelector<HTMLButtonElement>('.directory-only')!.click();
      expect(launch).toHaveBeenLastCalledWith(undefined);
      expect(select.value).toBe('codex');
      expect(localStorage.getItem('dev-dock-preferred-ai-tool')).toBe('codex');
      state.tools = [tools[0]];
      await nextTick();
      expect(select.value).toBe('');
      state.tools = [];
      await nextTick();
      expect(panel.root.querySelectorAll('option')).toHaveLength(1);
      const launches = launch.mock.calls.length;
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenCalledTimes(launches);
      expect(configure).toHaveBeenCalledTimes(2);
      expect(panel.root.textContent).toContain('请先在偏好设置配置并启用 AI 工具');
      panel.root.querySelector<HTMLButtonElement>('[aria-label="配置 AI 工具"]')!.click();
      expect(configure).toHaveBeenCalledTimes(3);
    } finally {
      panel.dispose();
    }
  });
  it('重新进入项目时恢复常用工具，已禁用的选择回退为仅目录', async () => {
    localStorage.setItem('dev-dock-preferred-ai-tool', 'codex');
    const panel = render({ render: () => h(AiLauncher, { tools, busy: false }) });
    try {
      expect(panel.root.querySelector<HTMLSelectElement>('select')!.value).toBe('codex');
    } finally {
      panel.dispose();
    }
    const disabled = render({ render: () => h(AiLauncher, { tools: [tools[0]], busy: false }) });
    try {
      expect(disabled.root.querySelector<HTMLSelectElement>('select')!.value).toBe('');
    } finally {
      disabled.dispose();
    }
  });
  it('交接任务期间禁用启动，避免重复打开', async () => {
    const launch = vi.fn();
    const panel = render({ render: () => h(AiLauncher, { tools, busy: true, onLaunch: launch }) });
    try {
      expect(panel.root.querySelector<HTMLSelectElement>('select')!.disabled).toBe(true);
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('AI 设置草稿不修改已保存配置，切换主题保留草稿，保存后可删除并禁用工具', async () => {
    const state = reactive({ settings: defaultConfig().settings });
    const save = vi.fn<(settings: Settings) => void>();
    const panel = render({
      render: () =>
        h(SettingsPage, {
          settings: state.settings,
          tools: { hbuilderxPath: '', terminals: [], jdks: [] },
          detecting: false,
          saving: false,
          busy: false,
          projectCount: 0,
          onSave: save,
        }),
    });
    try {
      const preset = Array.from(
        panel.root.querySelectorAll<HTMLButtonElement>('.ai-presets button'),
      ).find((button) => button.textContent?.includes('Claude Code'))!;
      preset.click();
      await nextTick();
      const command = panel.root.querySelector<HTMLInputElement>(
        '[aria-label="AI 工具 1 启动命令"]',
      )!;
      command.value = 'claude --profile example';
      command.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      expect(state.settings.aiTools).toEqual([]);
      state.settings = { ...state.settings, theme: 'blue' };
      await nextTick();
      expect(command.value).toBe('claude --profile example');
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[0][0].aiTools[0]).toMatchObject({
        name: 'Claude Code',
        command: 'claude --profile example',
        enabled: true,
      });
      expect(state.settings.aiTools).toEqual([]);
      const enabled = panel.root.querySelector<HTMLInputElement>('[aria-label="启用 AI 工具 1"]')!;
      enabled.click();
      await nextTick();
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[1][0].aiTools[0].enabled).toBe(false);
      panel.root.querySelector<HTMLButtonElement>('[aria-label="移除 AI 工具 1"]')!.click();
      await nextTick();
      expect(panel.root.querySelector('.ai-tool-row')).toBeNull();
    } finally {
      panel.dispose();
    }
  });
});
