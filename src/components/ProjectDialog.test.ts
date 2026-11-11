// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import { api } from '../api';
import { newProject, type Project } from '../types';

vi.mock('../api', () => ({
  desktop: true,
  api: { nodeVersions: vi.fn(), choosePath: vi.fn(), uniappScripts: vi.fn() },
}));
const versions = [
  {
    version: 'v22.22.2',
    nvmDir: '/local/nvm home',
    path: '/local/nvm home/versions/node/v22.22.2/bin/node',
  },
  {
    version: 'v16.20.2',
    nvmDir: '/local/nvm home',
    path: '/local/nvm home/versions/node/v16.20.2/bin/node',
  },
];
async function mountDialog(project: Project) {
  let saved: Project | undefined;
  const save = vi.fn((value: Project) => {
    saved = value;
  });
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(ProjectDialog, {
        project,
        tools: { hbuilderxPath: '', terminals: [], jdks: [] },
        existing: [],
        saving: false,
        detecting: false,
        onSave: save,
      }),
  });
  app.mount(root);
  await Promise.resolve();
  await nextTick();
  return {
    root,
    save,
    async select(value: string) {
      const select = root.querySelector<HTMLSelectElement>('#project-node-version')!;
      select.value = value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
    },
    async submit() {
      root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await nextTick();
      return saved;
    },
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
function nodeProject(): Project {
  return {
    ...newProject(),
    name: '开发项目',
    type: 'nodejs',
    path: '/local/project',
    platforms: ['web'],
  };
}
describe('Node 项目版本选择', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.nodeVersions).mockResolvedValue(versions);
  });
  it('每条终端命令单独保存环境变量，并拦截缺少等号或重复的格式', async () => {
    const dialog = await mountDialog(nodeProject());
    try {
      // 项目级不再提供环境变量输入。
      expect(dialog.root.querySelector('#project-env-vars')).toBeNull();
      const actionsTab = Array.from(dialog.root.querySelectorAll<HTMLButtonElement>('button')).find(
        (button) => button.textContent?.trim() === '运行与发行',
      )!;
      actionsTab.click();
      await nextTick();
      Array.from(dialog.root.querySelectorAll<HTMLButtonElement>('button'))
        .find((button) => button.textContent?.includes('添加'))!
        .click();
      await nextTick();
      const name = dialog.root.querySelector<HTMLInputElement>('.action-editor input')!;
      name.value = '开发服务';
      name.dispatchEvent(new Event('input', { bubbles: true }));
      const command = dialog.root.querySelector<HTMLTextAreaElement>('.action-editor textarea')!;
      command.value = 'npm run dev';
      command.dispatchEvent(new Event('input', { bubbles: true }));
      const env = dialog.root.querySelector<HTMLTextAreaElement>(
        '[aria-label="命令 1 的环境变量"]',
      )!;
      // 输入框下方给出普通值与 JSON 数组两种示例。
      expect(dialog.root.textContent).toContain(
        'ALERT_EMAIL_SMTP_HOST=smtp.qq.com;ALERT_EMAIL_SMTP_PORT=465',
      );
      expect(dialog.root.textContent).toContain(
        'AI_CONFIG=\'[{"name":"weixin","model":"DeepSeek"}]\'',
      );
      env.value =
        'AI_ALERT_EMAIL_ENABLED=false;ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;SMTP=smtp.qq.com;';
      env.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      expect(await dialog.submit()).toMatchObject({
        commands: [
          {
            command: 'npm run dev',
            envVars:
              'AI_ALERT_EMAIL_ENABLED=false;ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;SMTP=smtp.qq.com;',
          },
        ],
      });
      // 引号包裹的 JSON 值可包含分号，引号未闭合才报错。
      env.value = 'AI_CONFIG="[{\\"name\\":\\"weixin\\",\\"apiKey\\":\\"a;b\\"}]";OTHER=1';
      env.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      expect(await dialog.submit()).toMatchObject({
        commands: [{ envVars: expect.stringContaining('AI_CONFIG=') }],
      });
      env.value = 'AI_CONFIG="unterminated';
      env.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
      const closedCalls = dialog.save.mock.calls.length;
      await dialog.submit();
      expect(dialog.save).toHaveBeenCalledTimes(closedCalls);
      expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain(
        '环境变量引号未闭合',
      );
      for (const [value, message] of [
        ['NO_EQUALS', '环境变量缺少名称或等号'],
        ['1BAD=x', '环境变量名称无效'],
        ['A=1;A=2', '环境变量重复'],
      ]) {
        env.value = value;
        env.dispatchEvent(new Event('input', { bubbles: true }));
        await nextTick();
        const calls = dialog.save.mock.calls.length;
        await dialog.submit();
        expect(dialog.save).toHaveBeenCalledTimes(calls);
        expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain(
          `「开发服务」${message}`,
        );
      }
    } finally {
      dialog.dispose();
    }
  });
  it('Node 显示 Web 和小程序，uni-app 额外提供 App 并可启用 Web', async () => {
    const dialog = await mountDialog(nodeProject());
    try {
      expect(
        Array.from(dialog.root.querySelectorAll('.platform-option')).map((button) =>
          button.textContent?.replace('✓', '').trim(),
        ),
      ).toEqual(['Web', '小程序']);
      const type = dialog.root.querySelector<HTMLSelectElement>('#project-type')!;
      type.value = 'uniapp';
      type.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
      const buttons = Array.from(
        dialog.root.querySelectorAll<HTMLButtonElement>('.platform-option'),
      );
      expect(buttons.map((button) => button.textContent?.replace('✓', '').trim())).toEqual([
        'App 构建',
        'Web',
        '小程序',
      ]);
      buttons[1].click();
      await nextTick();
      expect(await dialog.submit()).toMatchObject({ type: 'uniapp', platforms: ['app', 'web'] });
    } finally {
      dialog.dispose();
    }
  });
  it('自动读取本地版本，保存所选版本及对应 nvm 目录', async () => {
    const dialog = await mountDialog(nodeProject());
    try {
      expect(api.nodeVersions).toHaveBeenCalledTimes(1);
      await dialog.select(JSON.stringify([versions[1].nvmDir, versions[1].version]));
      expect(await dialog.submit()).toMatchObject({
        nodeVersion: 'v16.20.2',
        nvmDir: '/local/nvm home',
      });
    } finally {
      dialog.dispose();
    }
  });
  it('选择终端默认版本时清除项目的 nvm 指定值', async () => {
    const dialog = await mountDialog({
      ...nodeProject(),
      nodeVersion: 'v22.22.2',
      nvmDir: '/local/nvm home',
    });
    try {
      await dialog.select('');
      expect(await dialog.submit()).toMatchObject({ nodeVersion: '', nvmDir: '' });
    } finally {
      dialog.dispose();
    }
  });
  it('已保存版本未检测到时保留选择，不自动改为其他版本', async () => {
    const dialog = await mountDialog({
      ...nodeProject(),
      nodeVersion: 'v14.21.3',
      nvmDir: '/other/nvm',
    });
    try {
      expect(dialog.root.textContent).toContain('v14.21.3（本机未检测到）');
      expect(await dialog.submit()).toMatchObject({
        nodeVersion: 'v14.21.3',
        nvmDir: '/other/nvm',
      });
    } finally {
      dialog.dispose();
    }
  });
});
