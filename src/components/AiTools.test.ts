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
describe('项目终端命令', () => {
  beforeEach(() => localStorage.clear());
  it('跟随全局默认，项目专用选择独立，目录按钮不改变选择', async () => {
    const state = reactive({ tools: [...tools], projectToolId: '', defaultToolId: 'claude' });
    const launch = vi.fn();
    const selectTool = vi.fn((id: string) => {
      state.projectToolId = id;
    });
    const panel = render({
      render: () =>
        h(AiLauncher, { ...state, busy: false, onLaunch: launch, onSelect: selectTool }),
    });
    try {
      expect(panel.root.querySelectorAll('option')).toHaveLength(3);
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenLastCalledWith(tools[0]);
      const select = panel.root.querySelector('select')!;
      select.value = 'codex';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
      expect(selectTool).toHaveBeenCalledWith('codex');
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenLastCalledWith(tools[1]);
      panel.root.querySelector<HTMLButtonElement>('.directory-only')!.click();
      expect(launch).toHaveBeenLastCalledWith(undefined);
      expect(state.projectToolId).toBe('codex');
      state.defaultToolId = '';
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenLastCalledWith(tools[1]);
      state.projectToolId = '';
      state.defaultToolId = 'claude';
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).toHaveBeenLastCalledWith(tools[0]);
    } finally {
      panel.dispose();
    }
  });
  it('未配置或项目命令失效时只提示配置，不误启全局命令', async () => {
    const state = reactive({
      tools: [...tools],
      projectToolId: 'missing',
      defaultToolId: 'claude',
    });
    const launch = vi.fn();
    const configure = vi.fn();
    const panel = render({
      render: () =>
        h(AiLauncher, { ...state, busy: false, onLaunch: launch, onConfigure: configure }),
    });
    try {
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).not.toHaveBeenCalled();
      expect(configure).toHaveBeenCalledOnce();
      expect(panel.root.textContent).toContain('已移除或禁用');
      state.tools = [];
      state.projectToolId = '';
      state.defaultToolId = '';
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      expect(launch).not.toHaveBeenCalled();
      expect(configure).toHaveBeenCalledTimes(2);
      expect(panel.root.textContent).toContain('请先在偏好设置配置并启用终端命令');
    } finally {
      panel.dispose();
    }
  });
  it('交接任务期间禁用执行和仅目录入口', async () => {
    const launch = vi.fn();
    const panel = render({
      render: () =>
        h(AiLauncher, {
          tools,
          projectToolId: '',
          defaultToolId: 'claude',
          busy: true,
          onLaunch: launch,
        }),
    });
    try {
      expect(panel.root.querySelector<HTMLSelectElement>('select')!.disabled).toBe(true);
      panel.root.querySelector<HTMLButtonElement>('.primary')!.click();
      panel.root.querySelector<HTMLButtonElement>('.directory-only')!.click();
      expect(launch).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('默认终端归入命令模块，行内按钮切换或取消唯一默认命令', async () => {
    const settings = { ...defaultConfig().settings, aiTools: tools.map((tool) => ({ ...tool })) };
    const save = vi.fn();
    const panel = render({
      render: () =>
        h(SettingsPage, {
          settings,
          tools: { hbuilderxPath: '', terminals: [], jdks: [] },
          detecting: false,
          saving: false,
          busy: false,
          projectCount: 0,
          onSave: save,
        }),
    });
    try {
      const cards = panel.root.querySelectorAll('.settings-tools-grid > .panel');
      expect(cards[0].textContent).not.toContain('默认终端');
      expect(cards[1].textContent).toContain('默认终端');
      expect(panel.root.querySelector('[aria-label="全局默认命令"]')).toBeNull();
      panel.root.querySelector<HTMLButtonElement>('[aria-label="设为默认终端命令 1"]')!.click();
      await nextTick();
      panel.root.querySelector<HTMLButtonElement>('[aria-label="设为默认终端命令 2"]')!.click();
      await nextTick();
      expect(panel.root.querySelectorAll('.ai-default-button.is-default')).toHaveLength(1);
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[0][0].defaultAiToolId).toBe('codex');
      expect(
        panel.root.querySelector<HTMLButtonElement>('[aria-label="设为默认终端命令 3"]')!.disabled,
      ).toBe(true);
      panel.root.querySelector<HTMLButtonElement>('[aria-label="取消默认终端命令 2"]')!.click();
      await nextTick();
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[1][0].defaultAiToolId).toBe('');
    } finally {
      panel.dispose();
    }
  });
  it('缺少命令时使用中文提示并聚焦，导出包含未保存的完整命令草稿', async () => {
    const settings = defaultConfig().settings;
    settings.aiTools = [{ id: 'custom', name: '自定义助手', command: '', enabled: true }];
    const save = vi.fn();
    const error = vi.fn();
    const exportSettings = vi.fn();
    const panel = render({
      render: () =>
        h(SettingsPage, {
          settings,
          tools: { hbuilderxPath: '', terminals: [], jdks: [] },
          detecting: false,
          saving: false,
          busy: false,
          projectCount: 0,
          onSave: save,
          onError: error,
          onExport: exportSettings,
        }),
    });
    try {
      const form = panel.root.querySelector('form')!;
      expect(form.noValidate).toBe(true);
      form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save).not.toHaveBeenCalled();
      expect(error).toHaveBeenCalledWith(
        '请为「自定义助手」填写执行命令，例如 claude、codex 或 git pull。',
      );
      const command = panel.root.querySelector<HTMLInputElement>(
        '[aria-label="终端命令 1 执行命令"]',
      )!;
      expect(document.activeElement).toBe(command);
      const exportButton = Array.from(
        panel.root.querySelectorAll<HTMLButtonElement>('button'),
      ).find((button) => button.textContent?.includes('导出 JSON'))!;
      exportButton.click();
      expect(exportSettings).not.toHaveBeenCalled();
      command.value = 'custom-ai --profile local';
      command.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      exportButton.click();
      expect(exportSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          aiTools: [
            {
              id: 'custom',
              name: '自定义助手',
              command: 'custom-ai --profile local',
              enabled: true,
            },
          ],
        }),
      );
      expect(settings.aiTools[0].command).toBe('');
      expect(save).not.toHaveBeenCalled();
    } finally {
      panel.dispose();
    }
  });
  it('终端命令设置草稿不修改已保存配置，切换主题保留草稿，保存后可删除并禁用工具', async () => {
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
        '[aria-label="终端命令 1 执行命令"]',
      )!;
      command.value = 'claude --profile example';
      command.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      expect(state.settings.aiTools).toEqual([]);
      panel.root.querySelector<HTMLButtonElement>('[aria-label="设为默认终端命令 1"]')!.click();
      await nextTick();
      expect(panel.root.querySelector('.ai-default-button')?.textContent).toBe('默认命令');
      state.settings = { ...state.settings, theme: 'blue' };
      await nextTick();
      expect(command.value).toBe('claude --profile example');
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[0][0].defaultAiToolId).toBe(save.mock.calls[0][0].aiTools[0].id);
      expect(save.mock.calls[0][0].aiTools[0]).toMatchObject({
        name: 'Claude Code',
        command: 'claude --profile example',
        enabled: true,
      });
      expect(state.settings.aiTools).toEqual([]);
      const enabled = panel.root.querySelector<HTMLInputElement>('[aria-label="启用终端命令 1"]')!;
      enabled.click();
      await nextTick();
      panel.root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      expect(save.mock.calls[1][0].aiTools[0].enabled).toBe(false);
      expect(save.mock.calls[1][0].defaultAiToolId).toBe('');
      panel.root.querySelector<HTMLButtonElement>('[aria-label="移除终端命令 1"]')!.click();
      await nextTick();
      expect(panel.root.querySelector('.ai-tool-row')).toBeNull();
    } finally {
      panel.dispose();
    }
  });
});
