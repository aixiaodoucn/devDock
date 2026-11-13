// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import { api } from '../api';
import { defaultConfig, newProject, type Project, type Settings, type NodeVersion } from '../types';

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
async function mountDialog(project: Project, settings?: Settings) {
  const currentSettings = ref(settings);
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
        settings: currentSettings.value,
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
    async setSettings(value: Settings) {
      currentSettings.value = value;
      await nextTick();
      await Promise.resolve();
      await nextTick();
    },
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
  it('公共目录为符号链接时接受原生返回的规范目录，不误判版本缺失', async () => {
    vi.mocked(api.nodeVersions).mockResolvedValueOnce([
      {
        manager: 'fnm',
        version: 'v22.22.2',
        nvmDir: '',
        fnmDir: '/resolved/fnm',
        path: '/resolved/fnm/node-versions/v22.22.2/installation/bin/node',
      },
    ]);
    const settings: Settings = {
      ...defaultConfig().settings,
      nodeManager: 'fnm',
      fnmDir: '/linked/fnm',
    };
    const dialog = await mountDialog(nodeProject(), settings);
    try {
      expect(api.nodeVersions).toHaveBeenCalledWith({ manager: 'fnm', dir: '/linked/fnm' });
      await dialog.select('v22.22.2');
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: 'v22.22.2',
        nvmDir: '',
        fnmDir: '',
      });
      expect(settings.fnmDir).toBe('/linked/fnm');
    } finally {
      dialog.dispose();
    }
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
  it.each(['nvm', 'fnm'] as const)(
    '仅从公共 %s 目录检测，项目只保存版本并跟随公共设置',
    async (manager) => {
      const dir = `/public/${manager} home`;
      const version: NodeVersion = {
        version: 'v20.19.0',
        manager,
        nvmDir: manager === 'nvm' ? dir : '',
        fnmDir: manager === 'fnm' ? dir : '',
        path: `${dir}/node`,
      };
      vi.mocked(api.nodeVersions).mockResolvedValue([version]);
      const settings = {
        ...defaultConfig().settings,
        nodeManager: manager,
        nvmDir: '/public/nvm home',
        fnmDir: '/public/fnm home',
      };
      const dialog = await mountDialog(
        { ...nodeProject(), nvmDir: '/old/project/nvm', fnmDir: '/old/project/fnm' },
        settings,
      );
      try {
        expect(api.nodeVersions).toHaveBeenCalledWith({ manager, dir });
        expect(dialog.root.querySelector('#project-node-manager')).toBeNull();
        expect(dialog.root.querySelector('[aria-label="版本数据目录"]')).toBeNull();
        expect(dialog.root.querySelector('[aria-label="选择版本数据目录"]')).toBeNull();
        expect(dialog.root.textContent).toContain('公共环境模式');
        expect(dialog.root.textContent).toContain(dir);
        expect(dialog.root.textContent).toContain('偏好设置');
        await dialog.select(version.version);
        expect(await dialog.submit()).toMatchObject({
          nodeManager: 'inherit',
          nodeVersion: version.version,
          nvmDir: '',
          fnmDir: '',
        });
        expect(settings.nvmDir).toBe('/public/nvm home');
        expect(settings.fnmDir).toBe('/public/fnm home');
        dialog.root.querySelector<HTMLButtonElement>('.version-select-row button')!.click();
        await Promise.resolve();
        expect(api.nodeVersions).toHaveBeenLastCalledWith({ manager, dir });
        expect(api.choosePath).not.toHaveBeenCalled();
      } finally {
        dialog.dispose();
      }
    },
  );
  it('公共管理器下可选择空版本，保存后使用终端默认环境', async () => {
    const dialog = await mountDialog(
      { ...nodeProject(), nodeVersion: versions[0].version, nvmDir: '/old/nvm' },
      { ...defaultConfig().settings, nodeManager: 'nvm', nvmDir: versions[0].nvmDir },
    );
    try {
      await dialog.select('');
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: '',
        nvmDir: '',
        fnmDir: '',
      });
    } finally {
      dialog.dispose();
    }
  });
  it('公共 system 不提供版本选择，保存清除旧项目版本覆盖', async () => {
    const dialog = await mountDialog(
      { ...nodeProject(), nodeManager: 'fnm', nodeVersion: 'v22.22.2', fnmDir: '/old/fnm' },
      { ...defaultConfig().settings, nodeManager: 'system' },
    );
    try {
      expect(api.nodeVersions).not.toHaveBeenCalled();
      expect(dialog.root.textContent).toContain('跟随终端默认环境');
      expect(dialog.root.querySelector('#project-node-manager')).toBeNull();
      expect(dialog.root.querySelector('#project-node-version')).toBeNull();
      expect(dialog.root.querySelector('[aria-label="版本数据目录"]')).toBeNull();
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: '',
        nvmDir: '',
        fnmDir: '',
      });
    } finally {
      dialog.dispose();
    }
  });
  it('公共环境未配置的新无版本项目使用终端默认环境，不扫描管理器', async () => {
    const project = nodeProject();
    const dialog = await mountDialog(project);
    try {
      expect(api.nodeVersions).not.toHaveBeenCalled();
      expect(dialog.root.querySelector('#project-node-manager')).toBeNull();
      expect(dialog.root.querySelector('#project-node-version')).toBeNull();
      expect(dialog.root.textContent).toContain('跟随终端默认环境');
      expect(await dialog.submit()).toMatchObject({
        nodeVersion: '',
        nvmDir: '',
        fnmDir: project.fnmDir,
      });
    } finally {
      dialog.dispose();
    }
  });
  it.each(['nvm', 'fnm'] as const)('无公共环境时保留旧 %s 元数据和目录', async (manager) => {
    const project = {
      ...nodeProject(),
      nodeManager: manager === 'nvm' ? ('' as const) : manager,
      nodeVersion: versions[0].version,
      nvmDir: manager === 'nvm' ? versions[0].nvmDir : '',
      fnmDir: manager === 'fnm' ? '/legacy/fnm' : '',
    };
    const dialog = await mountDialog(project, { ...defaultConfig().settings, nodeManager: '' });
    try {
      expect(api.nodeVersions).toHaveBeenCalledWith({
        manager,
        dir: manager === 'nvm' ? project.nvmDir : project.fnmDir,
      });
      expect(dialog.root.textContent).toContain('尚未设置公共环境，使用原项目配置');
      expect(dialog.root.querySelector('#project-node-manager')).toBeNull();
      expect(dialog.root.querySelector('[aria-label="版本数据目录"]')).toBeNull();
      expect(await dialog.submit()).toMatchObject({
        nodeManager: project.nodeManager,
        nodeVersion: project.nodeVersion,
        nvmDir: project.nvmDir,
        fnmDir: project.fnmDir,
      });
    } finally {
      dialog.dispose();
    }
  });
  it('未配置公共环境的 inherit 导入项目保留版本并提示配置，不猜测管理器', async () => {
    const project = {
      ...nodeProject(),
      nodeManager: 'inherit' as const,
      nodeVersion: 'v18.20.8',
      nvmDir: '',
      fnmDir: '',
    };
    const dialog = await mountDialog(project);
    try {
      expect(api.nodeVersions).not.toHaveBeenCalled();
      expect(dialog.root.querySelector('#project-node-manager')).toBeNull();
      expect(dialog.root.querySelector('#project-node-version')).toBeNull();
      expect(dialog.root.textContent).toContain('v18.20.8（本机未检测到）');
      expect(dialog.root.textContent).toContain('请到偏好设置配置');
      expect(await dialog.submit()).toMatchObject(project);
    } finally {
      dialog.dispose();
    }
  });
  it('公共模式切换只改变候选来源，保留项目已保存的精确版本', async () => {
    const fnm: NodeVersion = {
      version: 'v24.0.0',
      manager: 'fnm',
      nvmDir: '',
      fnmDir: '/public/fnm',
      path: '/public/fnm/node',
    };
    const dialog = await mountDialog(
      { ...nodeProject(), nodeVersion: versions[0].version, nvmDir: versions[0].nvmDir },
      { ...defaultConfig().settings, nodeManager: 'nvm', nvmDir: versions[0].nvmDir },
    );
    try {
      vi.mocked(api.nodeVersions).mockResolvedValue([fnm]);
      await dialog.setSettings({
        ...defaultConfig().settings,
        nodeManager: 'fnm',
        fnmDir: fnm.fnmDir,
      });
      expect(api.nodeVersions).toHaveBeenLastCalledWith({ manager: 'fnm', dir: fnm.fnmDir });
      const select = dialog.root.querySelector<HTMLSelectElement>('#project-node-version')!;
      expect(select.value).toBe(versions[0].version);
      expect(dialog.root.textContent).toContain(`${versions[0].version}（本机未检测到）`);
      await dialog.select(fnm.version);
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: fnm.version,
        nvmDir: '',
        fnmDir: '',
      });
    } finally {
      dialog.dispose();
    }
  });
  it('公共目录为空时检测默认目录，不把扫描结果目录写入项目或公共设置', async () => {
    const settings = { ...defaultConfig().settings, nodeManager: 'nvm' as const, nvmDir: '' };
    const dialog = await mountDialog(nodeProject(), settings);
    try {
      expect(api.nodeVersions).toHaveBeenCalledWith({ manager: 'nvm', dir: '' });
      await dialog.select(versions[0].version);
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: versions[0].version,
        nvmDir: '',
        fnmDir: '',
      });
      expect(settings.nvmDir).toBe('');
    } finally {
      dialog.dispose();
    }
  });
  it('空扫描保留已保存版本，仍允许切换为默认环境', async () => {
    vi.mocked(api.nodeVersions).mockResolvedValue([]);
    const dialog = await mountDialog(
      { ...nodeProject(), nodeManager: 'inherit', nodeVersion: 'v14.21.3' },
      { ...defaultConfig().settings, nodeManager: 'nvm', nvmDir: '/empty/nvm' },
    );
    try {
      expect(dialog.root.textContent).toContain('未发现已安装的 nvm');
      expect(dialog.root.textContent).toContain('v14.21.3（本机未检测到）');
      expect(await dialog.submit()).toMatchObject({
        nodeManager: 'inherit',
        nodeVersion: 'v14.21.3',
        nvmDir: '',
        fnmDir: '',
      });
      await dialog.select('');
      expect(await dialog.submit()).toMatchObject({ nodeManager: 'inherit', nodeVersion: '' });
    } finally {
      dialog.dispose();
    }
  });
  it.each(['lts/*', '22.22.2', 'v022.22.2'])(
    '拒绝旧配置中的非法精确版本 %s',
    async (nodeVersion) => {
      const dialog = await mountDialog({ ...nodeProject(), nodeVersion, nvmDir: '/local/nvm' });
      try {
        await dialog.submit();
        expect(dialog.save).not.toHaveBeenCalled();
        expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain(
          '精确 Node.js 版本',
        );
      } finally {
        dialog.dispose();
      }
    },
  );
  it('迟到的 nvm 刷新响应不能覆盖已切换的公共 fnm 列表', async () => {
    let finishNvm!: (items: NodeVersion[]) => void;
    const fnm: NodeVersion = {
      version: 'v24.0.0',
      manager: 'fnm',
      nvmDir: '',
      fnmDir: '/fnm',
      path: '/fnm/node',
    };
    vi.mocked(api.nodeVersions).mockImplementation((options) =>
      options?.manager === 'nvm'
        ? new Promise((resolve) => {
            finishNvm = resolve;
          })
        : Promise.resolve([fnm]),
    );
    const dialog = await mountDialog(nodeProject(), {
      ...defaultConfig().settings,
      nodeManager: 'nvm',
      nvmDir: versions[0].nvmDir,
    });
    try {
      await dialog.setSettings({ ...defaultConfig().settings, nodeManager: 'fnm', fnmDir: '/fnm' });
      finishNvm(versions);
      await Promise.resolve();
      await nextTick();
      const options = Array.from(
        dialog.root.querySelector<HTMLSelectElement>('#project-node-version')!.options,
      );
      expect(options.map((option) => option.value)).toContain(fnm.version);
      expect(dialog.root.textContent).not.toContain(versions[1].version);
    } finally {
      dialog.dispose();
    }
  });
  it('公共目录变更使在途旧目录响应失效', async () => {
    let finishOld!: (items: NodeVersion[]) => void;
    vi.mocked(api.nodeVersions).mockImplementation((options) =>
      options?.dir === '/old/nvm'
        ? new Promise((resolve) => {
            finishOld = resolve;
          })
        : Promise.resolve([]),
    );
    const dialog = await mountDialog(nodeProject(), {
      ...defaultConfig().settings,
      nodeManager: 'nvm',
      nvmDir: '/old/nvm',
    });
    try {
      await dialog.setSettings({
        ...defaultConfig().settings,
        nodeManager: 'nvm',
        nvmDir: '/new/nvm',
      });
      finishOld(versions);
      await Promise.resolve();
      await nextTick();
      expect(api.nodeVersions).toHaveBeenLastCalledWith({ manager: 'nvm', dir: '/new/nvm' });
      expect(dialog.root.textContent).not.toContain(versions[0].version);
      expect(await dialog.submit()).toMatchObject({ nodeManager: 'inherit', nodeVersion: '' });
    } finally {
      dialog.dispose();
    }
  });
});
