// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { beforeEach, expect, it, vi } from 'vitest';
import BackendWorkbench from './BackendWorkbench.vue';
import { api } from '../api';
import { listen } from '@tauri-apps/api/event';
import { newProject, type BackendGitOperation, type Project } from '../types';
const env = vi.hoisted(() => ({
  desktop: true,
  handler: null as null | ((event: { payload: { planId: string } }) => void),
  unlisten: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_: string, handler: typeof env.handler) => {
    env.handler = handler;
    return env.unlisten;
  }),
}));
vi.mock('../api', () => ({
  get desktop() {
    return env.desktop;
  },
  api: {
    logMcpStatus: vi.fn(async () => ({ enabled: false, endpoint: '' })),
    scmStatus: vi.fn(),
    scmProjects: vi.fn(),
    scmCreateProjectOptions: vi.fn(),
    scmCreateProject: vi.fn(),
    scmEditProjectOptions: vi.fn(),
    scmUpdateProject: vi.fn(),
    scmRemoveProjectModule: vi.fn(),
    scmProjectModules: vi.fn(),
    backendRepositoryStatuses: vi.fn(),
    backendRepositoryRemoteUrls: vi.fn(),
    prepareBackendGit: vi.fn(),
    prepareRemainingBackendGit: vi.fn(),
    executeBackendGit: vi.fn(),
    backendGitState: vi.fn(),
    backendGitOperations: vi.fn(),
    backendClearGitOperations: vi.fn(),
    openPath: vi.fn(),
    openBackendWorktreeInIdea: vi.fn(),
    openBackendWorktreeInWebStorm: vi.fn(),
    cancel: vi.fn(),
    abortBackendGit: vi.fn(),
    completeBackendGit: vi.fn(),
    resolveBackendPomConflict: vi.fn(),
    resolveBackendPomBlocks: vi.fn(),
  },
}));
async function flush() {
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
    await nextTick();
  }
}
function fixture(): Project {
  return {
    ...newProject(),
    id: 'example',
    name: 'Example',
    type: 'backend',
    scmDevelopmentProjectId: '123',
    scmTestProjectId: '456',
    subProjects: [
      {
        id: 'repo',
        name: 'Service',
        path: '/example/service',
        remote: 'origin',
        scmGitId: '42',
        scmModuleName: 'service',
        developmentBranch: 'develop',
        testBranch: 'test',
      },
    ],
  };
}
async function mount(initial = fixture()) {
  const project = ref(initial);
  const saving = ref(false);
  const busy = ref(false);
  const save = vi.fn();
  const edit = vi.fn();
  const openProjectScm = vi.fn();
  const task = vi.fn();
  const notice = vi.fn();
  const root = document.createElement('div');
  const app = createApp({
    render: () =>
      h(BackendWorkbench, {
        project: project.value,
        saving: saving.value,
        busy: busy.value,
        onSave: save,
        onEdit: edit,
        onOpenProjectScm: openProjectScm,
        onTask: task,
        onNotice: notice,
      }),
  });
  app.mount(root);
  await flush();
  return {
    project,
    saving,
    busy,
    save,
    edit,
    openProjectScm,
    task,
    notice,
    root,
    close: () => app.unmount(),
  };
}
async function ready(initial = fixture()) {
  const panel = await mount({
    ...initial,
    scmDevelopmentProjectId: initial.scmDevelopmentProjectId || '123',
  });
  click(panel.root, '加载模块');
  await flush();
  if (panel.save.mock.calls.length) {
    panel.project.value = panel.save.mock.calls.at(-1)![0];
    await flush();
  }
  panel.save.mockClear();
  return panel;
}
function click(root: HTMLElement, text: string) {
  const target = button(root, text);
  expect(target, text).toBeTruthy();
  target.click();
}
function input(root: HTMLElement, selector: string, value: string) {
  const field = root.querySelector<HTMLInputElement>(selector)!;
  expect(field.readOnly, selector).toBe(false);
  field.value = value;
  field.dispatchEvent(new Event('input'));
}
const createOptions = {
  users: [{ value: 'dev', label: '开发人员' }],
  defaultUser: 'dev',
  modules: [
    { value: '42', label: '支付服务' },
    { value: '99', label: '库存服务' },
  ],
  types: [{ value: 'feature', label: '需求' }],
};
it.each(['789', ''])('创建 SCM 项目后刷新列表并按返回 ID 处理开发项目（%s）', async (id) => {
  vi.mocked(api.scmCreateProjectOptions).mockResolvedValue(createOptions);
  vi.mocked(api.scmCreateProject).mockResolvedValue(id);
  vi.mocked(api.scmProjects).mockResolvedValue([]);
  const panel = await mount();
  try {
    click(panel.root, '创建 SCM 项目');
    await flush();
    expect(panel.root.querySelector('[aria-label="移除负责人 dev"]')).not.toBeNull();
    input(panel.root, '[name=name]', '新服务');
    input(panel.root, '[name=branchSuffix]', 'payment');
    input(panel.root, '[name=details]', '支付需求');
    const choices = panel.root.querySelector('[aria-label="选择模块"]')!;
    const selected = panel.root.querySelector('[aria-label="已选模块"]')!;
    for (const [query, label] of [
      ['支付', '支付服务'],
      ['99', '库存服务'],
    ]) {
      input(panel.root, '[name=moduleSearch]', query);
      await flush();
      const buttons = choices.querySelectorAll('button');
      expect(buttons).toHaveLength(1);
      expect(buttons[0].textContent).toContain(label);
      buttons[0].click();
      await flush();
      expect(buttons[0].disabled).toBe(true);
    }
    expect(selected.querySelectorAll('button')).toHaveLength(2);
    selected.querySelector<HTMLButtonElement>('[aria-label="移除支付服务"]')!.click();
    await flush();
    expect(selected.textContent).not.toContain('支付服务');
    expect(button(panel.root, '确认创建').disabled).toBe(false);
    expect(
      panel.root
        .querySelector<HTMLFormElement>('form[aria-label="创建 SCM 项目"]')
        ?.checkValidity(),
    ).toBe(true);
    panel.root
      .querySelector('form[aria-label="创建 SCM 项目"]')!
      .dispatchEvent(new Event('submit', { cancelable: true }));
    await flush();
    expect(api.scmCreateProject).toHaveBeenCalledExactlyOnceWith(
      {
        name: '新服务',
        branchSuffix: 'payment',
        details: '支付需求',
        projectType: 'feature',
        user: 'dev',
        modules: ['99'],
      },
      ['dev'],
    );
    expect(api.scmProjects).toHaveBeenCalledOnce();
    expect(panel.root.querySelector<HTMLInputElement>('#scm-development-project')?.value).toBe(
      id || '123',
    );
    if (id) {
      expect(api.scmProjectModules).toHaveBeenCalledWith(id);
      expect(panel.save.mock.calls[0]?.[0]).toMatchObject({
        scmDevelopmentProjectId: id,
        scmTestProjectId: '456',
      });
    } else {
      expect(panel.root.textContent).toContain('未返回项目 ID');
      expect(api.scmProjectModules).not.toHaveBeenCalled();
    }
  } finally {
    panel.close();
  }
});
const editOptions = {
  options: createOptions,
  input: {
    name: '支付',
    branchSuffix: 'pay',
    projectType: 'feature',
    details: '旧详情',
    user: 'dev',
    modules: [],
  },
  existingModules: ['42'],
  authors: ['dev', 'former'],
  revision: 'rev-1',
};
it.each([false, true])('保存修改时锁定开发项目并忽略过期回执（切换项目 %s）', async (switched) => {
  vi.mocked(api.scmEditProjectOptions).mockResolvedValue(editOptions);
  let finish!: () => void;
  vi.mocked(api.scmUpdateProject).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const panel = await mount({
    ...fixture(),
    scmDevelopmentProjectId: 'https://scm.example/projects/123',
  });
  try {
    click(panel.root, '修改 SCM 项目');
    await flush();
    expect(api.scmEditProjectOptions).toHaveBeenCalledExactlyOnceWith('123');
    const form = panel.root.querySelector<HTMLFormElement>('form[aria-label="修改 SCM 项目"]')!;
    expect(form.textContent).toContain('123');
    input(form, '[name=details]', '更新详情');
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(api.scmUpdateProject).toHaveBeenCalledExactlyOnceWith(
      '123',
      { ...editOptions.input, details: '更新详情' },
      editOptions.authors,
      'rev-1',
    );
    if (switched) panel.project.value = { ...fixture(), scmDevelopmentProjectId: '789' };
    await flush();
    finish();
    await flush();
    expect(panel.root.querySelector('form[aria-label="修改 SCM 项目"]')).toBeNull();
    if (switched) {
      expect(api.scmProjects).not.toHaveBeenCalled();
      expect(api.scmProjectModules).not.toHaveBeenCalled();
    } else {
      expect(api.scmProjects).toHaveBeenCalledOnce();
      expect(api.scmProjectModules).toHaveBeenCalledWith('123');
      expect(api.scmProjectModules).toHaveBeenCalledWith('456');
    }
  } finally {
    panel.close();
  }
});
it('保存失败保留草稿与版本，允许重试', async () => {
  vi.mocked(api.scmEditProjectOptions).mockResolvedValue(editOptions);
  vi.mocked(api.scmUpdateProject)
    .mockRejectedValueOnce(new Error('版本已变更'))
    .mockResolvedValueOnce();
  const panel = await mount();
  try {
    click(panel.root, '修改 SCM 项目');
    await flush();
    const form = panel.root.querySelector<HTMLFormElement>('form[aria-label="修改 SCM 项目"]')!;
    input(form, '[name=name]', '保留草稿');
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    await flush();
    expect(panel.root.querySelector('[role=alert]')?.textContent).toContain('版本已变更');
    expect(form.querySelector<HTMLInputElement>('[name=name]')?.value).toBe('保留草稿');
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    await flush();
    expect(api.scmUpdateProject).toHaveBeenCalledTimes(2);
    expect(api.scmUpdateProject).toHaveBeenLastCalledWith(
      '123',
      { ...editOptions.input, name: '保留草稿' },
      editOptions.authors,
      editOptions.revision,
    );
  } finally {
    panel.close();
  }
});
it('解除关联单独提交，保留其他草稿；后续保存使用新版本', async () => {
  vi.mocked(api.scmEditProjectOptions).mockResolvedValue(structuredClone(editOptions));
  vi.mocked(api.scmRemoveProjectModule).mockResolvedValue('after-removal');
  const panel = await mount();
  try {
    click(panel.root, '修改 SCM 项目');
    await flush();
    const form = panel.root.querySelector<HTMLFormElement>('form[aria-label="修改 SCM 项目"]')!;
    input(form, '[name=name]', '未保存草稿');
    form.querySelector<HTMLButtonElement>('[aria-label="解除关联 支付服务"]')!.click();
    await flush();
    expect(api.scmRemoveProjectModule).not.toHaveBeenCalled();
    panel.root
      .querySelector<HTMLButtonElement>('[role=alertdialog] [data-action=confirm]')!
      .click();
    await flush();
    expect(api.scmRemoveProjectModule).toHaveBeenCalledExactlyOnceWith('123', '42', 'rev-1');
    expect(form.querySelector('[aria-label="解除关联 支付服务"]')).toBeNull();
    expect(form.querySelector<HTMLInputElement>('[name=name]')?.value).toBe('未保存草稿');
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    await flush();
    expect(api.scmUpdateProject).toHaveBeenCalledWith(
      '123',
      expect.objectContaining({ name: '未保存草稿' }),
      editOptions.authors,
      'after-removal',
    );
  } finally {
    panel.close();
  }
});
it('切换工作台后丢弃晚到的编辑选项，即使开发项目 ID 相同', async () => {
  let finish!: (options: typeof editOptions) => void;
  vi.mocked(api.scmEditProjectOptions).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const panel = await mount();
  try {
    click(panel.root, '修改 SCM 项目');
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    finish(editOptions);
    await flush();
    expect(panel.root.querySelector('[name=branchSuffix]')).toBeNull();
  } finally {
    panel.close();
  }
});
it('未登录时隐藏创建和修改项目入口', async () => {
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: false,
    username: null,
    baseUrl: null,
    remembered: false,
    message: null,
  });
  const panel = await mount();
  try {
    expect(panel.root.querySelector('input[type=password]')).toBeNull();
    expect(panel.root.textContent).toContain('偏好设置 → MCP 服务 → 连接与权限');
    button(panel.root, '去登录').click();
    expect(panel.openProjectScm).toHaveBeenCalledOnce();
    expect(panel.root.textContent).not.toContain('创建 SCM 项目');
    expect(panel.root.textContent).not.toContain('修改 SCM 项目');
  } finally {
    panel.close();
  }
});
it('空后缀和空模块可以提交一次，切换项目后忽略创建回执', async () => {
  vi.mocked(api.scmCreateProjectOptions).mockResolvedValue(createOptions);
  let finish!: (id: string) => void;
  vi.mocked(api.scmCreateProject).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const panel = await mount();
  try {
    click(panel.root, '创建 SCM 项目');
    await flush();
    input(panel.root, '[name=name]', '服务');
    const form = panel.root.querySelector<HTMLFormElement>('form[aria-label="创建 SCM 项目"]')!;
    input(form, '[name=branchSuffix]', 'bad/branch');
    expect(form.checkValidity()).toBe(false);
    input(form, '[name=branchSuffix]', '');
    expect(form.checkValidity()).toBe(true);
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(api.scmCreateProject).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ branchSuffix: '', modules: [] }),
      ['dev'],
    );
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    finish('789');
    await flush();
    expect(api.scmProjects).not.toHaveBeenCalled();
    expect(panel.save).not.toHaveBeenCalled();
    expect(panel.notice).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('切换工作台后丢弃晚到的创建选项', async () => {
  let finish!: (options: typeof createOptions) => void;
  vi.mocked(api.scmCreateProjectOptions).mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const panel = await mount();
  try {
    click(panel.root, '创建 SCM 项目');
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    finish(createOptions);
    await flush();
    expect(panel.root.querySelector('[name=branchSuffix]')).toBeNull();
  } finally {
    panel.close();
  }
});
it('工作台只读取项目 SCM 状态，不显示登录卡片或加载本机 MCP 服务', async () => {
  const panel = await mount();
  try {
    expect(api.scmStatus).toHaveBeenCalledOnce();
    expect(panel.root.querySelector('[aria-label="项目 SCM 登录"]')).toBeNull();
    expect(panel.root.querySelector('[aria-label="本机 MCP 服务"]')).toBeNull();
    expect(api.logMcpStatus).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('未加载模块时，打开、更新本地配置、刷新和切换项目均不检查 Git', async () => {
  const panel = await mount();
  try {
    expect(button(panel.root, '刷新仓库').disabled).toBe(true);
    click(panel.root, '刷新仓库');
    panel.project.value.subProjects[0].path = '/example/new-path';
    await flush();
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    expect(api.backendRepositoryStatuses).not.toHaveBeenCalled();
    expect(api.backendRepositoryRemoteUrls).not.toHaveBeenCalled();
    expect(api.backendGitOperations).toHaveBeenLastCalledWith('other');
  } finally {
    panel.close();
  }
});
it('等 SCM 返回后只检查匹配的开发仓库，刷新沿用这个范围', async () => {
  const initial = fixture();
  const [repository] = initial.subProjects;
  initial.subProjects.push({ ...repository, id: 'other', scmGitId: '99' });
  const modules = await api.scmProjectModules('123');
  vi.mocked(api.scmProjectModules).mockClear();
  let resolve!: (value: typeof modules) => void;
  vi.mocked(api.scmProjectModules).mockImplementationOnce(
    () => new Promise((done) => (resolve = done)),
  );
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).toHaveBeenCalledWith('123');
    expect(api.backendRepositoryStatuses).not.toHaveBeenCalled();
    resolve(modules);
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenCalledExactlyOnceWith([repository]);
    expect(api.backendRepositoryRemoteUrls).not.toHaveBeenCalled();
    click(panel.root, '刷新仓库');
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenCalledTimes(2);
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith([repository]);
  } finally {
    panel.close();
  }
});
it('开发模块为空时不执行任何仓库检查', async () => {
  const initial = fixture();
  initial.subProjects[0].scmGitId = '';
  vi.mocked(api.scmProjectModules).mockResolvedValue([]);
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.backendRepositoryStatuses).not.toHaveBeenCalled();
    expect(api.backendRepositoryRemoteUrls).not.toHaveBeenCalled();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
  } finally {
    panel.close();
  }
});
it('只为无 Git ID 仓库查远端，手动刷新与操作结束复用范围，配置变化重查', async () => {
  const initial = fixture();
  const [direct] = initial.subProjects;
  const fallback = { ...direct, id: 'fallback', scmGitId: '' };
  const unrelated = { ...fallback, id: 'unrelated' };
  initial.subProjects.push(fallback, unrelated, { ...direct, id: 'other', scmGitId: '99' });
  vi.mocked(api.backendRepositoryRemoteUrls).mockResolvedValue({
    fallback: 'git@example.invalid:team/service.git',
    unrelated: 'git@example.invalid:team/unrelated.git',
  });
  const panel = await ready(initial);
  try {
    expect(api.backendRepositoryRemoteUrls).toHaveBeenCalledExactlyOnceWith([fallback, unrelated]);
    expect(api.backendRepositoryStatuses).toHaveBeenCalledExactlyOnceWith([direct, fallback]);
    const currentScope = panel.project.value.subProjects.slice(0, 2);
    expect(currentScope[1].scmGitId).toBe('42');
    click(panel.root, '刷新仓库');
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith(currentScope);
    vi.mocked(api.backendGitState).mockResolvedValue({
      planId: 'plan',
      taskId: 'task',
      status: 'success',
      repositories: [],
    });
    click(panel.root, '切换开发分支');
    await flush();
    click(panel.root, '确认执行计划');
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenCalledTimes(3);
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith(currentScope);
    expect(api.backendRepositoryRemoteUrls).toHaveBeenCalledTimes(1);

    const changed = { ...unrelated, remote: 'upstream' };
    panel.project.value.subProjects[2] = changed;
    await flush();
    expect(api.backendRepositoryRemoteUrls).toHaveBeenCalledTimes(2);
    expect(api.backendRepositoryRemoteUrls).toHaveBeenLastCalledWith([changed]);
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith(currentScope);
  } finally {
    panel.close();
  }
});
it('没有可匹配的开发模块 URL 时，无 Git ID 仓库不查远端', async () => {
  const initial = fixture();
  const [repo] = initial.subProjects;
  initial.subProjects.push({ ...repo, id: 'unknown', scmGitId: '' });
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' },
  ]);
  const panel = await ready(initial);
  try {
    expect(api.backendRepositoryRemoteUrls).not.toHaveBeenCalled();
    expect(api.backendRepositoryStatuses).toHaveBeenCalledExactlyOnceWith([repo]);
  } finally {
    panel.close();
  }
});
it.each(['SCM', 'remote'] as const)('切换项目后丢弃尚未返回的 %s 加载结果', async (phase) => {
  const initial = fixture();
  initial.subProjects[0].scmGitId = '';
  const modules = await api.scmProjectModules('123');
  let completeLoad!: () => void;
  if (phase === 'SCM') {
    vi.mocked(api.scmProjectModules).mockImplementationOnce(
      () => new Promise((resolve) => (completeLoad = () => resolve(modules))),
    );
  } else {
    vi.mocked(api.backendRepositoryRemoteUrls).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          completeLoad = () => resolve({ repo: 'git@example.invalid:team/service.git' });
        }),
    );
  }
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(completeLoad).toBeTypeOf('function');
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    expect(button(panel.root, '加载模块').disabled).toBe(false);
    completeLoad();
    await flush();
    expect(api.backendRepositoryStatuses).not.toHaveBeenCalled();
    if (phase === 'SCM') expect(api.backendRepositoryRemoteUrls).not.toHaveBeenCalled();
    expect(panel.save).not.toHaveBeenCalled();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
  } finally {
    panel.close();
  }
});
it('开发、测试、当前及未关联模块分支支持点击或键盘复制完整名称', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const branch = 'feature/' + 'example-branch_'.repeat(20);
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '456'
      ? [{ gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'test' }]
      : [
          { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch },
          { gitId: '43', moduleName: 'unlinked', gitRemoteUrl: null, branch: 'feature/unlinked' },
        ],
  );
  const panel = await ready();
  try {
    const development = panel.root.querySelector<HTMLElement>('.repository [data-development]')!;
    expect(development.getAttribute('role')).toBe('button');
    expect(development.tabIndex).toBe(0);
    development.click();
    await flush();
    expect(writeText).toHaveBeenLastCalledWith(branch);
    expect(panel.notice).toHaveBeenLastCalledWith('分支名已复制');
    const testing = panel.root.querySelector<HTMLElement>('.repository [data-test]')!;
    const space = new KeyboardEvent('keydown', { key: ' ', cancelable: true });
    testing.dispatchEvent(space);
    await flush();
    expect(space.defaultPrevented).toBe(true);
    expect(writeText).toHaveBeenLastCalledWith('test');
    panel.root.querySelector<HTMLElement>('[data-local-branch] code')!.click();
    await flush();
    expect(writeText).toHaveBeenLastCalledWith('develop');
    panel.root
      .querySelector<HTMLElement>('.repository-unlinked [data-development]')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await flush();
    expect(writeText).toHaveBeenLastCalledWith('feature/unlinked');
    expect(panel.save).not.toHaveBeenCalled();
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
    vi.unstubAllGlobals();
  }
});

it('Git 确认计划保留分支文字与标签，不提供点击复制', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const panel = await ready();
  try {
    click(panel.root, '推送开发分支到远程');
    await flush();
    const rows = [...panel.root.querySelectorAll<HTMLElement>('.plan-branch')];
    expect(rows.map((row) => row.querySelector('span')?.textContent)).toEqual([
      '当前分支：',
      '本地开发分支：',
      '目标远程分支：',
    ]);
    expect(rows.map((row) => row.querySelector('code')?.textContent)).toEqual([
      'develop',
      'develop',
      'origin/develop',
    ]);
    for (const row of rows) {
      const branch = row.querySelector<HTMLElement>('code')!;
      expect(branch.getAttribute('role')).toBeNull();
      expect(branch.classList.contains('branch-copyable')).toBe(false);
      branch.click();
    }
    await flush();
    expect(writeText).not.toHaveBeenCalled();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
    vi.unstubAllGlobals();
  }
});

it('未配置和游离 HEAD 等占位状态不提供复制入口', async () => {
  const writeText = vi.fn();
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const initial = fixture();
  initial.subProjects[0].testBranch = '';
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '456'
      ? []
      : [{ gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' }],
  );
  const panel = await ready(initial);
  try {
    const statuses = await vi.mocked(api.backendRepositoryStatuses).mock.results.at(-1)!.value;
    vi.mocked(api.backendRepositoryStatuses).mockResolvedValueOnce(
      statuses.map((status: Awaited<ReturnType<typeof api.backendRepositoryStatuses>>[number]) => ({
        ...status,
        currentBranch: '',
      })),
    );
    click(panel.root, '刷新仓库');
    await flush();
    const testing = panel.root.querySelector<HTMLElement>('[data-test]')!;
    const current = panel.root.querySelector<HTMLElement>('[data-local-branch] code')!;
    expect(testing.textContent).toBe('未配置');
    expect(current.textContent).toContain('游离 HEAD');
    for (const element of [testing, current]) {
      expect(element.getAttribute('role')).toBeNull();
      expect(element.getAttribute('tabindex')).toBeNull();
      element.click();
    }
    await flush();
    expect(writeText).not.toHaveBeenCalled();
  } finally {
    panel.close();
    vi.unstubAllGlobals();
  }
});

it('剪贴板拒绝时提示手动复制，不把失败报告为成功', async () => {
  const writeText = vi.fn().mockRejectedValue(new Error('permission denied'));
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const panel = await ready();
  try {
    panel.root.querySelector<HTMLElement>('[data-development]')!.click();
    await flush();
    expect(panel.notice).toHaveBeenCalledExactlyOnceWith('复制失败，请手动选择分支名复制。', true);
    expect(panel.save).not.toHaveBeenCalled();
  } finally {
    panel.close();
    vi.unstubAllGlobals();
  }
});

it('拉取当前分支只预检勾选仓库，不按开发分支切换或推送', async () => {
  const panel = await ready();
  try {
    click(panel.root, '取消全选');
    await flush();
    expect(button(panel.root, '拉取当前分支').disabled).toBe(true);
    click(panel.root, '全选');
    await flush();
    click(panel.root, '拉取当前分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledExactlyOnceWith(
      'example',
      [expect.objectContaining({ developmentBranch: 'develop' })],
      'pull-current',
      false,
      false,
    );
    expect(panel.root.querySelector('.plan')?.textContent).toContain('origin/main');
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    click(panel.root, '确认执行计划');
    await flush();
    expect(panel.task).toHaveBeenCalledWith('task', '拉取当前分支');
  } finally {
    panel.close();
  }
});

it('切换master不依赖测试目标，预检后确认执行且不采用合并选项', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '';
  initial.subProjects[0].testBranch = '';
  const panel = await ready(initial);
  try {
    for (const text of ['合并成功后推送测试分支']) {
      const checkbox = [...panel.root.querySelectorAll('label')]
        .find((label) => label.textContent?.includes(text))!
        .querySelector('input')!;
      checkbox.checked = true;
      checkbox.dispatchEvent(new Event('change'));
      await flush();
    }
    click(panel.root, '切换master分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledExactlyOnceWith(
      'example',
      [expect.objectContaining({ developmentBranch: 'develop', testBranch: '' })],
      'switch-master',
      false,
      false,
    );
    expect(panel.root.querySelector('.plan')?.textContent).toContain('切换master分支');
    expect(panel.root.querySelector('.plan')?.textContent).toContain('只允许快进');
    expect(panel.root.querySelector('.plan')?.textContent).toContain('执行时重新获取远端');
    expect([...panel.root.querySelectorAll('.plan-branch code')].at(-1)?.textContent).toBe(
      'master',
    );
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    click(panel.root, '确认执行计划');
    await flush();
    expect(api.executeBackendGit).toHaveBeenCalledExactlyOnceWith('plan');
    expect(panel.task).toHaveBeenCalledWith('task', '切换master分支');
  } finally {
    panel.close();
  }
});

it('切换master在工作区脏或保存期间不可执行', async () => {
  const panel = await ready();
  try {
    const statuses: Awaited<ReturnType<typeof api.backendRepositoryStatuses>> = await vi
      .mocked(api.backendRepositoryStatuses)
      .mock.results.at(-1)!.value;
    for (const condition of ['dirty', 'merging', 'rebasing'] as const) {
      vi.mocked(api.backendRepositoryStatuses).mockResolvedValueOnce(
        statuses.map((status) => ({ ...status, [condition]: true })),
      );
      click(panel.root, '刷新仓库');
      await flush();
      expect(button(panel.root, '切换master分支').disabled).toBe(true);
      click(panel.root, '切换master分支');
      expect(api.prepareBackendGit).not.toHaveBeenCalled();
    }
    panel.saving.value = true;
    await flush();
    expect(button(panel.root, '切换master分支').disabled).toBe(true);
  } finally {
    panel.close();
  }
});

it('开发与测试 SCM 选择候选后收起下拉且保留ID，普通输入不抢焦点', async () => {
  vi.mocked(api.scmProjects).mockResolvedValueOnce([
    { id: '101', name: 'Synthetic development', status: '待处理' },
    { id: '102', name: 'Synthetic testing', status: '测试环境' },
  ]);
  const panel = await mount();
  document.body.append(panel.root);
  try {
    click(panel.root, '加载项目列表');
    await flush();
    const fields = panel.root.querySelectorAll<HTMLInputElement>('[data-scm-project-input]');
    fields[0].focus();
    fields[0].value = '10';
    fields[0].dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    expect(document.activeElement).toBe(fields[0]);
    const developmentList = panel.root.querySelector(
      `#${fields[0].getAttribute('aria-controls')}`,
    )!;
    developmentList.querySelector<HTMLElement>('[data-project-id="101"]')!.click();
    await flush();
    expect(fields[0].getAttribute('aria-expanded')).toBe('false');
    expect(fields[0].value).toBe('101');
    fields[1].focus();
    await flush();
    const testingList = panel.root.querySelector(`#${fields[1].getAttribute('aria-controls')}`)!;
    testingList.querySelector<HTMLElement>('[data-project-id="102"]')!.click();
    await flush();
    expect(fields[1].getAttribute('aria-expanded')).toBe('false');
    expect(fields[1].value).toBe('102');
    expect(fields[0].value).toBe('101');
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    expect(panel.save).not.toHaveBeenCalled();
  } finally {
    panel.close();
    panel.root.remove();
  }
});

it('待处理和测试项目全部进入开发及测试来源的共享候选列表', async () => {
  const projects = Array.from({ length: 7 }, (_, index) => ({
    id: String(101 + index),
    name: `Synthetic project ${index + 1}`,
    status: index < 5 ? '待处理' : '测试环境',
  }));
  vi.mocked(api.scmProjects).mockResolvedValueOnce(projects);
  const panel = await mount();
  document.body.append(panel.root);
  try {
    click(panel.root, '加载项目列表');
    await flush();
    const fields = panel.root.querySelectorAll<HTMLInputElement>('[data-scm-project-input]');
    expect(fields).toHaveLength(2);
    expect(panel.root.querySelector('datalist')).toBeNull();
    for (const field of fields) {
      field.focus();
      await flush();
      const list = panel.root.querySelector(`#${field.getAttribute('aria-controls')}`)!;
      const options = [...list.querySelectorAll<HTMLElement>('[role="option"]')];
      expect(options.map((option) => option.dataset.projectId)).toEqual(
        projects.map((project) => project.id),
      );
      options.forEach((option, index) =>
        expect(option.textContent).toContain(projects[index].name),
      );
      field.blur();
      await flush();
    }
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
    panel.root.remove();
  }
});

it('仅开发 SCM ID 时只请求开发模块、清空旧测试分支并禁止测试操作', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '';
  initial.scmDevelopmentProjectId = '123';
  initial.subProjects[0].scmGitId = '42';
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: true,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  });
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'feature/new' },
  ]);
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).toHaveBeenCalledExactlyOnceWith('123');
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      developmentBranch: 'feature/new',
      testBranch: '',
    });
    expect(panel.save.mock.calls[0][0].scmTestProjectId).toBe('');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
    expect(panel.root.querySelector('[data-test]')).toBeNull();
    for (const name of ['切换到测试分支', '合并到测试分支']) {
      expect(button(panel.root, name).disabled).toBe(true);
      expect(button(panel.root, name).title).toContain('请填写测试 SCM 项目 ID');
      click(panel.root, name);
    }
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('外部新增子项目在开发源加载前不展示，且保留未保存开发 SCM ID', async () => {
  const initial = fixture();
  initial.subProjects = [];
  initial.scmDevelopmentProjectId = '';
  const panel = await mount(initial);
  try {
    input(panel.root, '[data-scm-project-input]', '123');
    await flush();
    panel.project.value = { ...initial, subProjects: fixture().subProjects };
    await flush();
    expect(panel.root.querySelector('.repository-title')).toBeNull();
    expect(panel.root.querySelector('[aria-label="SCM 项目与仓库"] .empty')).toBeNull();
    expect(panel.root.querySelector<HTMLInputElement>('[data-scm-project-input]')!.value).toBe(
      '123',
    );
    click(panel.root, '加载模块');
    await flush();
    expect(panel.root.querySelector<HTMLInputElement>('.repository-title input')!.checked).toBe(
      true,
    );
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0]).toMatchObject({
      scmDevelopmentProjectId: '123',
      subProjects: fixture().subProjects,
    });
  } finally {
    panel.close();
  }
});
it('填写非法测试 ID 时加载显示明确错误，且不请求模块或保存', async () => {
  const initial = fixture();
  initial.scmDevelopmentProjectId = '123';
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: true,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  });
  const panel = await mount(initial);
  try {
    input(panel.root, '.scm-projects label:nth-child(2) input', 'invalid');
    await flush();
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('测试 SCM 项目 ID 无效');
    expect(panel.save).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('测试 SCM 项目 ID 无效');
  } finally {
    panel.close();
  }
});
it('填写测试 ID 但模块为空时不能按开发-only应用', async () => {
  const initial = fixture();
  initial.scmDevelopmentProjectId = '123';
  initial.scmTestProjectId = '456';
  initial.subProjects[0].scmGitId = '42';
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: true,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  });
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '123'
      ? [{ gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'feature/new' }]
      : [],
  );
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).toHaveBeenCalledWith('456');
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0].testBranch).toBe('');
    expect(panel.root.textContent).toContain('唯一测试分支');
  } finally {
    panel.close();
  }
});
it.each(['', 'develop'])('测试分支为 %j 时合并仍被拦截，开发切换不受影响', async (branch) => {
  const initial = fixture();
  initial.subProjects[0].testBranch = branch;
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '456' && !branch
      ? []
      : [
          {
            gitId: '42',
            moduleName: 'service',
            gitRemoteUrl: null,
            branch: id === '456' ? branch : 'develop',
          },
        ],
  );
  const panel = await ready(initial);
  try {
    click(panel.root, '合并到测试分支');
    await flush();
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    expect(button(panel.root, '合并到测试分支').title).toContain('明确且不同的开发分支和测试分支');
    click(panel.root, '切换开发分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      expect.any(Array),
      'switch',
      false,
      false,
    );
  } finally {
    panel.close();
  }
});
it('外部增删和重命名以新配置为准，新增仓库重新匹配后自动保存', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'removed', name: 'Removed' });
  const panel = await ready(initial);
  try {
    const updated = fixture();
    updated.subProjects[0].name = 'Renamed';
    updated.subProjects.push({
      ...fixture().subProjects[0],
      id: 'new',
      name: 'New',
      developmentBranch: 'old-new',
    });
    panel.project.value = updated;
    await flush();
    expect(
      [...panel.root.querySelectorAll('.repository-title strong')].map((node) => node.textContent),
    ).toEqual(['Renamed', 'New']);
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects).toEqual([
      expect.objectContaining({ id: 'repo', name: 'Renamed', developmentBranch: 'develop' }),
      expect.objectContaining({ id: 'new', name: 'New', developmentBranch: 'develop' }),
    ]);
    expect(panel.root.textContent).toContain('等待保存确认');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
  } finally {
    panel.close();
  }
});
it.each(['path', 'remote'] as const)('外部改变仓库 %s 后重新匹配 SCM 并自动保存', async (field) => {
  const panel = await ready();
  try {
    const updated = fixture();
    updated.subProjects[0][field] = field === 'path' ? '/example/another' : 'external-remote';
    updated.subProjects[0].developmentBranch = 'external-stale';
    panel.project.value = updated;
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith([
      expect.objectContaining({ [field]: updated.subProjects[0][field] }),
    ]);
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      [field]: updated.subProjects[0][field],
      developmentBranch: 'develop',
      scmGitId: '42',
    });
    expect(button(panel.root, '切换开发分支').title).toContain('等待保存确认');
  } finally {
    panel.close();
  }
});
it('连续外部原地更新使用独立快照并移除失效计划', async () => {
  const panel = await ready();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    expect(panel.root.textContent).toContain('确认执行计划');
    panel.project.value.subProjects[0].name = 'Renamed';
    await flush();
    expect(panel.root.textContent).not.toContain('确认执行计划');
    panel.project.value.subProjects.push({
      ...fixture().subProjects[0],
      id: 'new',
      name: 'New',
      developmentBranch: 'old-new',
    });
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0].name).toBe('Renamed');
    expect(panel.save.mock.calls[0][0].subProjects).toHaveLength(2);
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    panel.project.value.subProjects[0].testBranch = 'external-test';
    await flush();
    expect(panel.root.querySelector<HTMLElement>('[data-test]')!.textContent).toBe('external-test');
  } finally {
    panel.close();
  }
});
it('新增仓库加入选择、删除仓库剔除，保留取消选择且排除错误目录', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'removed', name: 'Removed' });
  const panel = await ready(initial);
  try {
    const checkbox = panel.root.querySelector<HTMLInputElement>('.repository-title input')!;
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event('change'));
    await flush();
    vi.mocked(api.backendRepositoryStatuses).mockResolvedValue([
      {
        id: 'new',
        currentBranch: 'develop',
        head: 'abc',
        dirty: false,
        merging: false,
        rebasing: false,
        remoteUrl: '',
        branches: [],
        root: '/example/service',
        error: '',
      },
      {
        id: 'docs',
        currentBranch: '',
        head: '',
        dirty: false,
        merging: false,
        rebasing: false,
        remoteUrl: '',
        branches: [],
        root: '',
        error: '不是 Git 仓库',
      },
    ]);
    panel.project.value = {
      ...fixture(),
      subProjects: [
        ...fixture().subProjects,
        { ...fixture().subProjects[0], id: 'new', name: 'New' },
        { ...fixture().subProjects[0], id: 'docs', name: 'Docs' },
      ],
    };
    await flush();
    const checkboxes = panel.root.querySelectorAll<HTMLInputElement>('.repository-title input');
    expect([...checkboxes].map((node) => node.checked)).toEqual([false, true]);
    click(panel.root, '切换开发分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      [expect.objectContaining({ id: 'new' })],
      'switch',
      false,
      false,
    );
  } finally {
    panel.close();
  }
});
it('同项目外部更新后忽略尚未完成的旧预检', async () => {
  let resolve!: (value: Awaited<ReturnType<typeof api.prepareBackendGit>>) => void;
  vi.mocked(api.prepareBackendGit).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const panel = await ready();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    panel.project.value = { ...fixture(), subProjects: [] };
    await flush();
    resolve({
      planId: 'stale',
      projectId: 'example',
      operation: 'switch',
      push: false,
      repositories: [],
    });
    await flush();
    expect(panel.root.textContent).not.toContain('确认执行计划');
    expect(panel.root.querySelector('[role="status"]')).toBeNull();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
  } finally {
    panel.close();
  }
});
it('外部结构更新不冒充精确保存回执，失败保留 SCM 草稿和新增仓库', async () => {
  const panel = await ready();
  try {
    vi.mocked(api.scmProjectModules).mockResolvedValue([
      { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'feature/scm' },
    ]);
    click(panel.root, '加载模块');
    await flush();
    const pending = panel.save.mock.calls[0][0] as Project;
    panel.saving.value = true;
    await flush();
    panel.project.value = {
      ...pending,
      subProjects: [
        ...pending.subProjects,
        { ...fixture().subProjects[0], id: 'new', name: 'New' },
      ],
    };
    await flush();
    expect(panel.root.textContent).toContain('等待保存确认');
    expect(panel.root.textContent).not.toContain('映射已保存');
    click(panel.root, '切换开发分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.saving.value = false;
    await flush();
    expect(panel.root.textContent).toContain('保存未完成');
    expect(panel.root.querySelector<HTMLElement>('[data-development]')!.textContent).toBe(
      'feature/scm',
    );
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(2);
  } finally {
    panel.close();
  }
});
it('普通目录记录不参与 Git 选择，也不阻断其他仓库的 SCM 映射', async () => {
  const initial = fixture();
  initial.scmDevelopmentProjectId = '123';
  initial.scmTestProjectId = '456';
  const directory = {
    ...initial.subProjects[0],
    id: 'docs',
    name: '资料目录',
    path: '/example/docs',
    developmentBranch: '',
    testBranch: '',
  };
  initial.subProjects.push(directory);
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: true,
    baseUrl: 'https://example.invalid',
    username: 'example',
    remembered: false,
    message: null,
  });
  vi.mocked(api.backendRepositoryStatuses).mockResolvedValue([
    {
      id: 'repo',
      currentBranch: 'develop',
      head: 'abc',
      dirty: false,
      merging: false,
      rebasing: false,
      remoteUrl: 'git@example.invalid:team/service.git',
      branches: ['develop', 'test'],
      root: '/example/service',
      error: '',
    },
    {
      id: 'docs',
      currentBranch: '',
      head: '',
      dirty: false,
      merging: false,
      rebasing: false,
      remoteUrl: '',
      branches: [],
      root: '',
      error: '不是 Git 仓库',
    },
  ]);
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) => [
    {
      gitId: '42',
      moduleName: 'service',
      gitRemoteUrl: 'https://example.invalid/team/service.git',
      branch: id === '123' ? 'feature/new' : 'testing',
    },
  ]);
  const panel = await mount(initial);
  try {
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
    click(panel.root, '加载模块');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    const saved = panel.save.mock.calls[0][0] as Project;
    expect(saved.subProjects[0].developmentBranch).toBe('feature/new');
    expect(saved.subProjects[1]).toEqual(directory);
  } finally {
    panel.close();
  }
});

it.each([true, false])(
  '合并计划区分快进同步与本地未推送提交：syncTestBranch=%s',
  async (syncTestBranch) => {
    const prepare = vi.mocked(api.prepareBackendGit).getMockImplementation()!;
    vi.mocked(api.prepareBackendGit).mockImplementation(async (...args) => {
      const plan = await prepare(...args);
      return {
        ...plan,
        repositories: plan.repositories.map((item) => ({
          ...item,
          targetSha: 'local-test',
          remoteSha: 'remote-test',
          syncTestBranch,
        })),
      };
    });
    const panel = await ready();
    try {
      click(panel.root, '合并到测试分支');
      await flush();
      const text = panel.root.querySelector('.plan')?.textContent || '';
      expect(text.includes('先快进同步远端最新代码')).toBe(syncTestBranch);
      expect(text.includes('测试分支已有本地未推送提交')).toBe(!syncTestBranch);
      expect(api.executeBackendGit).not.toHaveBeenCalled();
    } finally {
      panel.close();
    }
  },
);

it.each([
  ['落后', 'remote-dev', true, '开发分支落后于远端'],
  ['领先', 'remote-dev', false, '开发分支已有本地未推送提交'],
  ['一致', 'local-dev', false, '开发分支已与远端同步'],
  ['无远端开发分支', '', false, '使用本地已提交的开发代码'],
  ['旧计划', undefined, undefined, ''],
] as const)(
  '合并计划展示开发分支状态：%s，确认前不执行同步',
  async (_state, developmentRemoteSha, syncDevelopmentBranch, notice) => {
    const prepare = vi.mocked(api.prepareBackendGit).getMockImplementation()!;
    vi.mocked(api.prepareBackendGit).mockImplementation(async (...args) => {
      const plan = await prepare(...args);
      return {
        ...plan,
        repositories: plan.repositories.map((item) => ({
          ...item,
          sourceSha: 'local-dev',
          developmentRemoteSha,
          syncDevelopmentBranch,
        })),
      };
    });
    const panel = await ready();
    try {
      click(panel.root, '合并到测试分支');
      await flush();
      const detail = panel.root.querySelector('[data-development-sync]');
      if (notice) expect(detail?.textContent).toContain(notice);
      else expect(detail).toBeNull();
      expect(api.executeBackendGit).not.toHaveBeenCalled();
      click(panel.root, '取消计划');
      click(panel.root, '切换开发分支');
      await flush();
      expect(panel.root.querySelector('[data-development-sync]')).toBeNull();
    } finally {
      panel.close();
    }
  },
);

it.each(['test', 'development'] as const)('显示两侧 POM 并明确选择 %s', async (side) => {
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    {
      planId: 'paused',
      taskId: 'task',
      status: 'paused',
      repositories: [
        {
          repoId: 'repo',
          status: 'error',
          message: '',
          worktreePath: '/example/worktree',
          pomConflictFiles: [
            {
              path: 'module/pom.xml',
              blocks: [
                {
                  line: 10,
                  testContent: '<version>1-test</version>',
                  developmentContent: '<version>2-dev</version>',
                },
              ],
            },
          ],
          pomConflict: {
            path: 'module/pom.xml',
            testBranch: 'test',
            developmentBranch: 'develop',
            testContent: '<version>1-test</version>',
            developmentContent: '<version>2-dev</version>',
          },
        },
      ],
    },
  ]);
  const panel = await mount();
  try {
    expect(panel.root.textContent).toContain('module/pom.xml');
    expect(panel.root.textContent).toContain('<version>1-test</version>');
    expect(panel.root.textContent).toContain('<version>2-dev</version>');
    expect(api.resolveBackendPomConflict).not.toHaveBeenCalled();
    click(panel.root, side === 'test' ? '采用测试分支版本' : '采用开发分支版本');
    await flush();
    expect(api.resolveBackendPomConflict).toHaveBeenCalledExactlyOnceWith('paused', 'repo', side);
  } finally {
    panel.close();
  }
});

it('按文件和工作树行号展示多个 POM 冲突块，提供手工处理入口', async () => {
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    {
      planId: 'paused',
      taskId: 'task',
      status: 'paused',
      repositories: [
        {
          repoId: 'repo',
          status: 'error',
          message: '',
          worktreePath: '/example/worktree',
          pomConflictFiles: [
            {
              path: 'pom.xml',
              branchDiff:
                '@@ -12 +12 @@\n-<version>1</version>\n+<version>2</version>\n@@ -42 +42 @@\n-<artifactId>old</artifactId>\n+<artifactId>new</artifactId>\n',
              diffContexts: { '@@ -12 +12 @@': '依赖 org.example:client' },
              blocks: [
                {
                  line: 12,
                  context: '依赖 org.example:client',
                  testContent: '<version>1</version>',
                  developmentContent: '<version>2</version>',
                },
                { line: 32, testContent: '', developmentContent: '<module>api</module>' },
              ],
            },
            {
              path: 'api/pom.xml',
              blocks: [{ line: 7, testContent: 'test line', developmentContent: 'dev line' }],
            },
          ],
        },
      ],
    },
  ]);
  const panel = await mount();
  try {
    const preview = panel.root.querySelector('.pom-conflict')!;
    expect(preview.textContent).toContain('api/pom.xml');
    expect(
      [...preview.querySelectorAll('[data-pom-block]')].map((block) =>
        block.getAttribute('data-pom-block'),
      ),
    ).toEqual(['12', '32', '7']);
    expect(preview.textContent).toContain('<version>1</version>');
    expect(preview.textContent).toContain('<version>2</version>');
    expect(preview.textContent).toContain('此侧无内容');
    expect(preview.querySelector('[data-pom-block="12"] .pom-context')?.textContent).toBe(
      '依赖 org.example:client',
    );
    expect(preview.querySelector('.pom-diff .pom-context')?.textContent).toBe(
      '依赖 org.example:client',
    );
    expect(
      [...preview.querySelectorAll('.pom-diff-range .pom-diff-text')].map(
        (line) => line.textContent,
      ),
    ).toEqual(['@@ -12 +12 @@', '@@ -42 +42 @@']);
    expect(preview.querySelector('.pom-diff .pom-test')?.textContent).toBe('-<version>1</version>');
    expect(preview.querySelector('.pom-diff .pom-development')?.textContent).toBe(
      '+<version>2</version>',
    );
    expect(preview.querySelector('.pom-diff')?.textContent).toContain(
      '<artifactId>new</artifactId>',
    );
    expect(preview.textContent).not.toContain('采用测试分支版本');
    click(panel.root, '打开工作树');
    expect(api.openPath).toHaveBeenCalledExactlyOnceWith('/example/worktree');
    expect(api.openBackendWorktreeInIdea).not.toHaveBeenCalled();
    click(panel.root, '在 IDEA 中打开');
    await flush();
    expect(api.openBackendWorktreeInIdea).toHaveBeenCalledExactlyOnceWith('paused', 'repo');
    expect(api.openBackendWorktreeInWebStorm).not.toHaveBeenCalled();
    expect(button(panel.root, '在 IDEA 中打开').nextElementSibling).toBe(
      button(panel.root, '在 WebStorm 中打开工作树'),
    );
    click(panel.root, '在 WebStorm 中打开工作树');
    await flush();
    expect(api.openBackendWorktreeInWebStorm).toHaveBeenCalledExactlyOnceWith('paused', 'repo');
  } finally {
    panel.close();
  }
});

it.each([false, true])(
  '逐块混合选择两侧，全部选齐才应用（剩余手工冲突：%s）',
  async (manualRemaining) => {
    vi.mocked(api.backendGitOperations).mockResolvedValue([
      {
        planId: 'paused',
        taskId: 'task',
        status: 'paused',
        repositories: [
          {
            repoId: 'repo',
            status: 'error',
            message: '',
            worktreePath: '/example/worktree',
            pomBlocksResolution: { push: manualRemaining, manualRemaining },
            pomConflictFiles: [
              {
                path: 'pom.xml',
                blocks: [
                  { line: 12, testContent: 'test one', developmentContent: 'dev one' },
                  { line: 32, testContent: 'test two', developmentContent: 'dev two' },
                ],
              },
            ],
          },
        ],
      },
    ]);
    const panel = await mount();
    try {
      const submit = button(
        panel.root,
        manualRemaining ? '应用所选 POM 内容' : '应用所选内容并提交',
      );
      expect(submit.disabled).toBe(true);
      const choose = (line: number, side: string) => {
        const radio = panel.root.querySelector<HTMLInputElement>(
          `[data-pom-block="${line}"] input[value="${side}"]`,
        )!;
        radio.checked = true;
        radio.dispatchEvent(new Event('change', { bubbles: true }));
      };
      choose(12, 'test');
      await flush();
      expect(submit.disabled).toBe(true);
      choose(32, 'development');
      await flush();
      expect(submit.disabled).toBe(false);
      expect(api.resolveBackendPomBlocks).not.toHaveBeenCalled();
      submit.click();
      await flush();
      expect(api.resolveBackendPomBlocks).toHaveBeenCalledExactlyOnceWith('paused', 'repo', [
        { path: 'pom.xml', line: 12, side: 'test' },
        { path: 'pom.xml', line: 32, side: 'development' },
      ]);
    } finally {
      panel.close();
    }
  },
);

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.scmProjects).mockResolvedValue([]);
  env.desktop = true;
  vi.mocked(listen).mockImplementation(async (_event, handler) => {
    env.handler = handler as typeof env.handler;
    return env.unlisten;
  });
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: true,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  });
  vi.mocked(api.backendRepositoryRemoteUrls).mockImplementation(async (repositories) =>
    Object.fromEntries(
      repositories.map((repo) => [repo.id, 'git@example.invalid:team/service.git']),
    ),
  );
  vi.mocked(api.backendRepositoryStatuses).mockImplementation(async (repositories) =>
    repositories.map((repo) => ({
      id: repo.id,
      currentBranch: 'develop',
      head: 'abc',
      dirty: false,
      merging: false,
      rebasing: false,
      remoteUrl: 'git@example.invalid:team/service.git',
      branches: ['develop', 'test'],
      root: repo.path,
      error: '',
    })),
  );
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) => [
    {
      gitId: '42',
      moduleName: 'service',
      gitRemoteUrl: 'https://example.invalid/team/service.git',
      branch: id === '456' ? 'test' : 'develop',
    },
  ]);
  vi.mocked(api.backendGitOperations).mockResolvedValue([]);
  vi.mocked(api.prepareBackendGit).mockImplementation(
    async (projectId, repositories, operation, push, keepTestPom) => ({
      planId: 'plan',
      projectId,
      operation,
      push: !!push,
      keepTestPom: !!keepTestPom,
      repositories: repositories.map((repository) => ({
        repository,
        root: repository.path,
        commonDir: '',
        currentBranch: operation === 'push-dev' ? repository.developmentBranch : 'main',
        sourceSha: 'abc123',
        targetBranch:
          operation === 'pull-current'
            ? 'main'
            : operation === 'switch-master'
              ? 'master'
              : operation === 'switch' || operation === 'push-dev'
                ? repository.developmentBranch
                : repository.testBranch,
        targetSha: 'def',
        remoteSha: 'def',
        createTracking: false,
      })),
    }),
  );
  vi.mocked(api.backendGitState).mockResolvedValue({
    planId: 'plan',
    taskId: 'task',
    status: 'running',
    repositories: [],
  });
  vi.mocked(api.executeBackendGit).mockResolvedValue('task');
});
it('预检不执行，push 默认关闭，确认后执行且与保存互斥', async () => {
  const panel = await ready();
  click(panel.root, '切换开发分支');
  await flush();
  expect(api.prepareBackendGit).toHaveBeenCalledWith(
    'example',
    expect.any(Array),
    'switch',
    false,
    false,
  );
  expect(api.executeBackendGit).not.toHaveBeenCalled();
  click(panel.root, '确认执行计划');
  await flush();
  expect(api.executeBackendGit).toHaveBeenCalledWith('plan');
  expect(button(panel.root, '加载模块').disabled).toBe(true);
  panel.close();
});
it('映射字段只读且没有保存映射按钮，SCM 自动保存不导出凭据并等待父回执', async () => {
  vi.mocked(api.scmStatus).mockResolvedValueOnce({
    connected: true,
    baseUrl: 'https://example.invalid',
    username: 'fixture-user',
    remembered: false,
    message: null,
  });
  const panel = await ready();
  try {
    expect(button(panel.root, '保存映射')).toBeUndefined();
    expect(panel.root.querySelector('[data-save]')).toBeNull();
    expect(panel.root.querySelector('input[data-remote], input[data-scm-git-id]')).toBeNull();
    expect(panel.root.querySelector('code[data-remote]')?.textContent).toBe('origin');
    expect(panel.root.querySelector('code[data-scm-git-id]')?.textContent).toBe('42');
    expect(panel.root.querySelector('input[data-development], input[data-test]')).toBeNull();
    expect(panel.root.querySelector('code[data-development]')?.textContent).toBe('develop');
    expect(panel.root.querySelector('code[data-test]')?.textContent).toBe('test');
    expect(panel.root.querySelector('[name="password"]')).toBeNull();
    expect(panel.root.textContent).not.toContain('fixture-user');
    vi.mocked(api.scmProjectModules).mockResolvedValue([
      { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'feature/new' },
    ]);
    click(panel.root, '加载模块');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    expect(JSON.stringify(panel.save.mock.calls)).not.toMatch(
      /password|session|username|fixture-user/,
    );
    expect(panel.root.textContent).toContain('等待保存确认');
    click(panel.root, '切换开发分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
  } finally {
    panel.close();
  }
});
it('加载将 SCM 链接规范成 ID，非法链接不请求模块或保存', async () => {
  const panel = await ready();
  try {
    input(
      panel.root,
      '.scm-projects label:nth-child(1) input',
      'https://example.invalid/projects/123',
    );
    input(panel.root, '.scm-projects label:nth-child(2) input', ' 456 ');
    await flush();
    click(panel.root, '加载模块');
    await flush();
    expect(panel.save.mock.calls[0][0]).toMatchObject({
      scmDevelopmentProjectId: '123',
      scmTestProjectId: '456',
    });
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
    input(
      panel.root,
      '.scm-projects label:nth-child(1) input',
      'https://example.invalid/projects/no-id',
    );
    await flush();
    vi.mocked(api.scmProjectModules).mockClear();
    click(panel.root, '加载模块');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('请输入纯数字 SCM 项目 ID');
    expect(panel.root.textContent).not.toContain('等待保存确认');
  } finally {
    panel.close();
  }
});
it('预览不调用 native 和事件监听', async () => {
  env.desktop = false;
  const panel = await ready();
  expect(api.backendRepositoryStatuses).not.toHaveBeenCalled();
  expect(api.backendGitOperations).not.toHaveBeenCalled();
  expect(api.scmStatus).not.toHaveBeenCalled();
  expect(listen).not.toHaveBeenCalled();
  panel.close();
});
it('切换项目忽略旧计划并清理监听', async () => {
  let resolve!: (plan: Awaited<ReturnType<typeof api.prepareBackendGit>>) => void;
  vi.mocked(api.prepareBackendGit).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const panel = await ready();
  click(panel.root, '切换开发分支');
  await flush();
  panel.project.value = { ...fixture(), id: 'other' };
  await flush();
  resolve({
    planId: 'stale',
    projectId: 'example',
    operation: 'switch',
    push: false,
    repositories: [],
  });
  await flush();
  expect(panel.root.textContent).not.toContain('确认执行计划');
  panel.close();
  expect(env.unlisten).toHaveBeenCalled();
});
it('监听异步注册晚于卸载时立即清理', async () => {
  let register!: (stop: () => void) => void;
  vi.mocked(listen).mockReturnValueOnce(
    new Promise((resolve) => {
      register = resolve;
    }),
  );
  const panel = await ready();
  panel.close();
  register(env.unlisten);
  await flush();
  expect(env.unlisten).toHaveBeenCalledTimes(1);
});
it('事件乱序响应不会把成功覆盖为旧的运行状态', async () => {
  const panel = await ready();
  click(panel.root, '切换开发分支');
  await flush();
  click(panel.root, '确认执行计划');
  await flush();
  let older!: (value: Awaited<ReturnType<typeof api.backendGitState>>) => void;
  vi.mocked(api.backendGitState).mockReturnValueOnce(
    new Promise((resolve) => {
      older = resolve;
    }),
  );
  env.handler!({ payload: { planId: 'plan' } });
  vi.mocked(api.backendGitState).mockResolvedValueOnce({
    planId: 'plan',
    taskId: 'task',
    status: 'success',
    repositories: [],
  });
  env.handler!({ payload: { planId: 'plan' } });
  await flush();
  older({ planId: 'plan', taskId: 'task', status: 'running', repositories: [] });
  await flush();
  expect(panel.root.textContent).toContain('执行结果 · 已完成');
  const select = panel.root.querySelector<HTMLSelectElement>('.history-toolbar select')!;
  expect(select.selectedOptions[0]?.textContent).toBe('暂无待处理操作');
  expect(select.disabled).toBe(true);
  panel.close();
});

it('清除前须明确确认，成功后关闭已删除的结果并刷新待处理列表', async () => {
  vi.mocked(api.backendClearGitOperations).mockResolvedValue(['plan']);
  const panel = await ready();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    vi.mocked(api.backendGitState).mockResolvedValueOnce({
      planId: 'plan',
      taskId: 'task',
      status: 'success',
      repositories: [],
    });
    click(panel.root, '确认执行计划');
    await flush();
    vi.mocked(api.backendGitOperations)
      .mockClear()
      .mockResolvedValue([
        {
          planId: 'recoverable',
          taskId: 'paused-task',
          status: 'paused',
          repositories: [
            { repoId: 'repo', status: 'error', message: '', worktreePath: '/worktree' },
          ],
        },
      ]);

    click(panel.root, '清除记录');
    await flush();
    expect(panel.root.textContent).toContain('选中的待处理操作一经清除便无法续跑');
    expect(panel.root.textContent).toContain('保留工作区的操作将保留');
    expect(api.backendClearGitOperations).not.toHaveBeenCalled();
    click(panel.root, '取消清除');
    await flush();
    expect(panel.root.textContent).toContain('执行结果 · 已完成');
    expect(api.backendClearGitOperations).not.toHaveBeenCalled();

    click(panel.root, '清除记录');
    await flush();
    click(panel.root, '确认清除');
    await flush();
    expect(api.backendClearGitOperations).toHaveBeenCalledExactlyOnceWith('example', undefined);
    expect(api.backendGitOperations).toHaveBeenCalledExactlyOnceWith('example');
    expect(panel.notice).toHaveBeenLastCalledWith('已清除 1 条 Git 操作记录');
    expect(panel.root.querySelector('.operation')).toBeNull();
    const select = panel.root.querySelector<HTMLSelectElement>('.history-toolbar select')!;
    expect(select.value).toBe('');
    expect([...select.options].map((option) => option.value)).toEqual(['', 'recoverable']);

    click(panel.root, '刷新记录');
    await flush();
    expect(api.backendGitOperations).toHaveBeenCalledTimes(2);
    expect(select.value).toBe('recoverable');
    expect(panel.root.textContent).toContain('执行结果 · 已暂停，需处理');
  } finally {
    panel.close();
  }
});

it('清除下拉选中的待续跑操作后保留其他待续跑选项，并保持结果为空', async () => {
  const unselected: BackendGitOperation = {
    planId: 'unselected',
    taskId: 'unselected-task',
    status: 'paused',
    repositories: [{ repoId: 'repo', status: 'pending', message: '', worktreePath: '' }],
  };
  const selected = { ...unselected, planId: 'selected', taskId: 'selected-task' };
  vi.mocked(api.backendGitOperations)
    .mockResolvedValueOnce([unselected, selected])
    .mockResolvedValueOnce([unselected]);
  vi.mocked(api.backendClearGitOperations).mockResolvedValue(['selected']);
  vi.mocked(api.prepareRemainingBackendGit).mockResolvedValue({
    planId: 'continuation',
    projectId: 'example',
    operation: 'switch',
    push: false,
    keepTestPom: false,
    repositories: [],
  });
  const panel = await mount();
  try {
    const select = panel.root.querySelector<HTMLSelectElement>('.history-toolbar select')!;
    expect(select.value).toBe('unselected');
    select.value = 'selected';
    select.dispatchEvent(new Event('change'));
    await flush();
    expect(panel.root.textContent).toContain('执行结果 · 已暂停，需处理');
    click(panel.root, '继续');
    await flush();
    expect(api.prepareRemainingBackendGit).toHaveBeenCalledExactlyOnceWith('selected');
    expect(panel.root.querySelector('.plan')).not.toBeNull();

    click(panel.root, '清除记录');
    await flush();
    click(panel.root, '确认清除');
    await flush();

    expect(api.backendClearGitOperations).toHaveBeenCalledExactlyOnceWith('example', 'selected');
    expect([...select.options].map((option) => option.value)).toEqual(['', 'unselected']);
    expect(select.value).toBe('');
    expect(panel.root.querySelector('.operation')).toBeNull();
    expect(panel.root.querySelector('.plan')).toBeNull();
  } finally {
    panel.close();
  }
});

it.each([
  {
    worktreePath: '/worktree',
    retainedNotice: '选中的操作仍有保留工作区，已保留该记录。',
  },
  { worktreePath: '', retainedNotice: '选中的操作仍需保留，未清除。' },
])(
  '根据实际删除 ID 保留选中项并提示（工作区：$worktreePath）',
  async ({ worktreePath, retainedNotice }) => {
    vi.mocked(api.backendGitOperations).mockResolvedValue([
      {
        planId: 'retained',
        taskId: 'retained-task',
        status: 'paused',
        repositories: [{ repoId: 'repo', status: 'error', message: '保留的结果', worktreePath }],
      },
    ]);
    vi.mocked(api.backendClearGitOperations).mockResolvedValue(['other-plan']);
    const panel = await mount();
    try {
      click(panel.root, '清除记录');
      await flush();
      click(panel.root, '确认清除');
      await flush();
      expect(api.backendClearGitOperations).toHaveBeenCalledExactlyOnceWith('example', 'retained');
      expect(panel.root.querySelector<HTMLSelectElement>('.history-toolbar select')?.value).toBe(
        'retained',
      );
      expect(panel.root.querySelector('.operation')?.textContent).toContain('保留的结果');
      expect(panel.notice).toHaveBeenLastCalledWith(`已清除 1 条 Git 操作记录。${retainedNotice}`);
    } finally {
      panel.close();
    }
  },
);

it('没有待处理操作仍可确认清除，没有可删记录时提示零条结果', async () => {
  vi.mocked(api.backendClearGitOperations).mockResolvedValue([]);
  const panel = await mount();
  try {
    expect(panel.root.querySelector('.operation')).toBeNull();
    expect(button(panel.root, '清除记录').disabled).toBe(false);
    click(panel.root, '清除记录');
    await flush();
    click(panel.root, '确认清除');
    await flush();
    expect(api.backendClearGitOperations).toHaveBeenCalledExactlyOnceWith('example', undefined);
    expect(panel.notice).toHaveBeenLastCalledWith('没有可清除的 Git 操作记录');
  } finally {
    panel.close();
  }
});

it('切换项目后不刷新或清除新项目的结果，也不显示旧项目的清除提示', async () => {
  let complete!: (ids: string[]) => void;
  vi.mocked(api.backendClearGitOperations).mockReturnValueOnce(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  const panel = await mount();
  try {
    click(panel.root, '清除记录');
    await flush();
    click(panel.root, '确认清除');
    await flush();
    expect(button(panel.root, '确认清除').disabled).toBe(true);
    vi.mocked(api.backendGitOperations).mockResolvedValueOnce([
      {
        planId: 'current',
        taskId: 'current-task',
        status: 'paused',
        repositories: [{ repoId: 'repo', status: 'pending', message: '', worktreePath: '' }],
      },
    ]);
    panel.project.value = { ...fixture(), id: 'other' };
    await flush();
    complete(['old-plan']);
    await flush();
    expect(api.backendClearGitOperations).toHaveBeenCalledExactlyOnceWith('example', undefined);
    expect(vi.mocked(api.backendGitOperations).mock.calls).toEqual([['example'], ['other']]);
    expect(panel.root.textContent).toContain('执行结果 · 已暂停，需处理');
    expect(button(panel.root, '确认清除')).toBeUndefined();
    expect(button(panel.root, '清除记录').disabled).toBe(false);
    expect(panel.notice).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('保存中与预检中均禁止重复提交操作，合并默认不推送', async () => {
  const panel = await ready();
  panel.saving.value = true;
  await flush();
  click(panel.root, '切换开发分支');
  expect(api.prepareBackendGit).not.toHaveBeenCalled();
  panel.saving.value = false;
  await flush();
  let resolve!: (value: Awaited<ReturnType<typeof api.prepareBackendGit>>) => void;
  vi.mocked(api.prepareBackendGit).mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  click(panel.root, '合并到测试分支');
  await flush();
  click(panel.root, '合并到测试分支');
  expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
  expect(api.prepareBackendGit).toHaveBeenLastCalledWith(
    'example',
    expect.any(Array),
    'merge',
    false,
    false,
  );
  resolve({
    planId: 'plan',
    projectId: 'example',
    operation: 'merge',
    push: false,
    repositories: [],
  });
  await flush();
  panel.close();
});
it.each(['prepared', 'paused'] as const)(
  '未启动任务的 %s 预检记录不显示为历史操作',
  async (status) => {
    vi.mocked(api.backendGitOperations).mockResolvedValue([
      {
        planId: 'unstarted-plan',
        taskId: '',
        status,
        repositories: [{ repoId: 'repo', status: 'pending', message: '', worktreePath: '' }],
      },
    ]);
    const panel = await ready();
    try {
      const select = panel.root.querySelector<HTMLSelectElement>('.history-toolbar select')!;
      expect(select.selectedOptions[0]?.textContent).toBe('暂无待处理操作');
      expect(select.options).toHaveLength(1);
      expect(panel.root.querySelector('.operation')).toBeNull();
      expect(panel.root.textContent).not.toContain('读取历史操作');
      expect(api.executeBackendGit).not.toHaveBeenCalled();
    } finally {
      panel.close();
    }
  },
);

it('暂停批次显示中文结果、仓库统计和未执行原因，记录名称不显示计划编号', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'next', name: 'Next service' });
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    {
      planId: 'opaque-plan-id',
      taskId: 'task',
      status: 'paused',
      repositories: [
        { repoId: 'repo', status: 'error', message: '目标分支检查失败', worktreePath: '' },
        { repoId: 'next', status: 'pending', message: '', worktreePath: '' },
      ],
    },
  ]);
  const panel = await ready(initial);
  try {
    const result = panel.root.querySelector('.operation')!;
    expect(result.textContent).toContain('执行结果 · 已暂停，需处理');
    expect(result.textContent).toContain('共 2 个仓库 · 成功 0 · 失败 1 · 未执行 1');
    expect(result.textContent).toContain('目标分支检查失败');
    expect(result.textContent).toContain('批次已暂停，该仓库尚未执行；请重新预检后操作。');
    expect(
      [...result.querySelectorAll('.result-heading span')].map((el) => el.textContent),
    ).toEqual(['失败', '未执行']);
    expect(result.textContent).not.toMatch(/pending|paused/);
    const options = panel.root.querySelector('.history-toolbar select')!.textContent;
    expect(options).toContain('Service、Next service · 已暂停，需处理');
    expect(options).not.toContain('opaque-plan-id');
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('中止冲突仓库后可继续预检剩余仓库，并确认执行新计划', async () => {
  const initial = fixture();
  const next = { ...initial.subProjects[0], id: 'next', name: 'Next service' };
  initial.subProjects.push(next);
  const paused = {
    planId: 'paused',
    taskId: 'old-task',
    status: 'paused',
    repositories: [
      { repoId: 'repo', status: 'error', message: '', worktreePath: '/example/worktree' },
      { repoId: 'next', status: 'pending', message: '', worktreePath: '' },
    ],
  } as const;
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    { ...paused, repositories: [...paused.repositories] },
  ]);
  vi.mocked(api.backendGitState).mockResolvedValueOnce({
    ...paused,
    repositories: [
      { ...paused.repositories[0], status: 'aborted', worktreePath: '' },
      paused.repositories[1],
    ],
  });
  vi.mocked(api.prepareRemainingBackendGit).mockResolvedValue({
    planId: 'remaining',
    projectId: initial.id,
    operation: 'merge',
    push: true,
    keepTestPom: true,
    repositories: [
      {
        repository: next,
        root: next.path,
        commonDir: `${next.path}/.git`,
        currentBranch: 'develop',
        targetBranch: 'test',
        createTracking: false,
        sourceSha: 'source',
        targetSha: 'target',
        remoteSha: 'target',
      },
    ],
  });
  const panel = await mount(initial);
  try {
    expect(button(panel.root, '继续')).toBeUndefined();
    expect(button(panel.root, '采用测试分支版本')).toBeUndefined();
    click(panel.root, '中止该次合并');
    await flush();
    click(panel.root, '继续');
    await flush();
    expect(api.prepareRemainingBackendGit).toHaveBeenCalledExactlyOnceWith('paused');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    const plan = panel.root.querySelector('.plan')!;
    expect([...plan.querySelectorAll('li strong')].map((item) => item.textContent)).toEqual([
      'Next service',
    ]);
    expect(plan.textContent).toContain('将推送测试分支');
    expect(plan.textContent).toContain('可逐处选择 pom.xml 冲突内容');
    click(panel.root, '取消计划');
    await flush();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    click(panel.root, '继续');
    await flush();
    click(panel.root, '确认执行计划');
    await flush();
    expect(api.executeBackendGit).toHaveBeenCalledExactlyOnceWith('remaining');
    expect(panel.root.querySelector('option[value="paused"]')).toBeNull();
  } finally {
    panel.close();
  }
});

it('执行中的仓库显示等待执行说明，不显示内部 pending 状态', async () => {
  vi.mocked(api.backendGitState).mockResolvedValue({
    planId: 'plan',
    taskId: 'task',
    status: 'running',
    repositories: [{ repoId: 'repo', status: 'pending', message: '', worktreePath: '' }],
  });
  const panel = await ready();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    click(panel.root, '确认执行计划');
    await flush();
    const result = panel.root.querySelector('.operation')!;
    expect(result.textContent).toContain('执行结果 · 执行中');
    expect(result.textContent).toContain('等待执行');
    expect(result.textContent).toContain('正在等待执行，尚未修改该仓库。');
    expect(result.textContent).not.toContain('pending');
  } finally {
    panel.close();
  }
});

it('已取消但保留工作树的操作仍提供处理入口', async () => {
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    {
      planId: 'cancelled-plan',
      taskId: 'task',
      status: 'cancelled',
      repositories: [
        { repoId: 'repo', status: 'cancelled', message: '', worktreePath: '/example/worktree' },
      ],
    },
  ]);
  const panel = await ready();
  try {
    expect(panel.root.textContent).toContain('执行结果 · 已取消');
    expect(panel.root.textContent).toContain('有保留的工作树');
    expect(button(panel.root, '打开工作树')).toBeTruthy();
    expect(button(panel.root, '中止该次合并')).toBeTruthy();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('预检失败可重试，恢复 paused 只显示结果不直接执行', async () => {
  vi.mocked(api.backendGitOperations).mockResolvedValue([
    {
      planId: 'old',
      taskId: 'old-task',
      status: 'paused',
      repositories: [
        { repoId: 'repo', status: 'error', message: 'conflict', worktreePath: '/example/worktree' },
      ],
    },
  ]);
  const panel = await ready();
  expect(panel.root.textContent).toContain('已手工解决，完成处理');
  expect(api.executeBackendGit).not.toHaveBeenCalled();
  vi.mocked(api.prepareBackendGit).mockRejectedValueOnce(new Error('fetch failed'));
  click(panel.root, '切换开发分支');
  await flush();
  expect(panel.root.textContent).toContain('fetch failed');
  click(panel.root, '已手工解决，完成处理');
  expect(api.completeBackendGit).not.toHaveBeenCalled();
  const confirmation = [...panel.root.querySelectorAll('label')]
    .find((label) => label.textContent?.includes('已手工提交并确认工作树干净'))!
    .querySelector('input')!;
  confirmation.checked = true;
  confirmation.dispatchEvent(new Event('change'));
  await flush();
  click(panel.root, '已手工解决，完成处理');
  await flush();
  expect(api.completeBackendGit).toHaveBeenCalledWith('old', 'repo');
  expect(api.executeBackendGit).not.toHaveBeenCalled();
  panel.close();
});

it('单一 SCM 项目与仓库按开发范围展示，关联仓库不重复且未关联模块不可选', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '456';
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '123'
      ? [
          { gitId: '42', moduleName: 'service', branch: 'feature/current', gitRemoteUrl: null },
          { gitId: '43', moduleName: 'missing-test', branch: 'feature/other', gitRemoteUrl: null },
        ]
      : [
          {
            gitId: '42',
            moduleName: 'renamed-service',
            branch: 'test/current',
            gitRemoteUrl: null,
          },
          { gitId: '99', moduleName: 'service', branch: 'test/unrelated', gitRemoteUrl: null },
          { gitId: null, moduleName: 'unknown', branch: 'test/unknown', gitRemoteUrl: null },
        ],
  );
  const panel = await ready(initial);
  try {
    const section = panel.root.querySelector('section[aria-label="SCM 项目与仓库"]')!;
    expect(panel.root.querySelectorAll('section[aria-label="SCM 项目与仓库"]')).toHaveLength(1);
    expect(panel.root.textContent).not.toContain('SCM 分支映射');
    expect(panel.root.textContent).not.toContain('本地子仓库');
    expect(panel.root.querySelector('table')).toBeNull();
    expect(section.querySelectorAll('.repository')).toHaveLength(1);
    expect(section.querySelectorAll('.repository-title strong')).toHaveLength(1);
    expect(section.querySelector('.repository-title strong')?.textContent).toBe('Service');
    expect(section.querySelector('.repository')?.textContent).toContain('模块：service');
    expect(section.textContent).not.toContain('编辑子项目');
    expect(section.querySelector('input[data-development], input[data-test]')).toBeNull();
    expect(section.querySelector<HTMLElement>('.repository [data-development]')?.textContent).toBe(
      'feature/current',
    );
    expect(section.querySelector<HTMLElement>('.repository [data-test]')?.textContent).toBe(
      'test/current',
    );
    expect(section.querySelectorAll('[data-local-branch]')).toHaveLength(1);
    expect(section.querySelector('[data-local-branch]')?.textContent).toContain('develop');
    expect(section.querySelector('[data-local-branch]')?.textContent).not.toContain('尚未切换');
    const unlinked = section.querySelector('.repository-unlinked')!;
    expect(unlinked.textContent).toContain('missing-test');
    expect(unlinked.textContent).toContain('未关联');
    expect(unlinked.querySelector('[type="checkbox"]')).toBeNull();
    expect(unlinked.querySelector('input[data-scm-git-id]')).toBeNull();
    expect(unlinked.querySelector('code[data-scm-git-id]')?.textContent).toBe('43');
    expect(unlinked.querySelector<HTMLElement>('[data-development]')?.textContent).toBe(
      'feature/other',
    );
    expect(unlinked.querySelector<HTMLElement>('[data-test]')?.textContent).toBe('未配置');
    click(unlinked as HTMLElement, '关联本地目录');
    expect(panel.edit).toHaveBeenCalledOnce();
    expect(section.textContent).not.toContain('renamed-service');
    expect(section.textContent).not.toContain('test/unrelated');
    expect(section.textContent).not.toContain('test/unknown');
    expect(section.querySelectorAll('[data-test]')).toHaveLength(2);
    expect(panel.project.value.subProjects[0].testBranch).toBe('test/current');
  } finally {
    panel.close();
  }
});

it('仓库卡片本地分支随刷新更新，游离 HEAD 不冒充分支', async () => {
  const panel = await ready();
  try {
    const local = () => panel.root.querySelector('[data-local-branch]')?.textContent;
    expect(local()).toContain('develop');
    expect(local()).not.toContain('尚未切换');
    const statuses: Awaited<ReturnType<typeof api.backendRepositoryStatuses>> = await vi
      .mocked(api.backendRepositoryStatuses)
      .mock.results.at(-1)!.value;
    vi.mocked(api.backendRepositoryStatuses).mockResolvedValueOnce(
      statuses.map((status) => ({ ...status, currentBranch: 'feature/local-current' })),
    );
    click(panel.root, '刷新仓库');
    await flush();
    expect(local()).toContain('feature/local-current');
    expect(local()).not.toContain('尚未切换');
    vi.mocked(api.backendRepositoryStatuses).mockResolvedValueOnce(
      statuses.map((status) => ({ ...status, currentBranch: '', head: 'abc123456789' })),
    );
    click(panel.root, '刷新仓库');
    await flush();
    expect(local()).toContain('游离 HEAD (abc12345)');
    expect(local()).not.toContain('尚未切换');
    expect(panel.root.querySelector('.repository-heading [data-remote]')).not.toBeNull();
    expect(panel.root.querySelector('.repository-heading [data-scm-git-id]')).not.toBeNull();
    expect(panel.root.querySelectorAll('.mapping-fields .mapping-branch')).toHaveLength(2);
  } finally {
    panel.close();
  }
});

it('开发源子集只展示与预检关联仓库，自动保存保留全部配置', async () => {
  const initial = fixture();
  const unrelated = { ...initial.subProjects[0], id: 'other', name: 'Other', scmGitId: '99' };
  initial.subProjects.push(unrelated);
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', branch: 'feature/subset', gitRemoteUrl: null },
  ]);
  const panel = await mount(initial);
  try {
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
    click(panel.root, '加载模块');
    await flush();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(1);
    expect(panel.root.textContent).toContain('关联 1 / 配置 2');
    expect(panel.root.textContent).toContain('已选 1 个仓库');
    expect(panel.save.mock.calls[0][0].subProjects).toEqual([
      expect.objectContaining({ id: 'repo', developmentBranch: 'feature/subset' }),
      unrelated,
    ]);
    expect(panel.root.textContent).toContain('等待保存确认');
    click(panel.root, '切换开发分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    click(panel.root, '切换开发分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      [expect.objectContaining({ id: 'repo' })],
      'switch',
      false,
      false,
    );
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('修改测试源保留开发范围并要求加载测试目标，修改开发源立刻清空范围与计划', async () => {
  const panel = await ready();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    expect(panel.root.querySelector('.plan')).not.toBeNull();
    input(panel.root, '.scm-projects label:nth-child(2) input', '789');
    await flush();
    expect(panel.root.querySelector('.plan')).toBeNull();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(1);
    expect(panel.save).not.toHaveBeenCalled();
    expect(button(panel.root, '合并到测试分支').title).toContain('请重新加载模块并等待自动保存');
    expect(button(panel.root, '切换开发分支').disabled).toBe(true);
    click(panel.root, '加载模块');
    await flush();
    panel.project.value = panel.save.mock.calls.at(-1)![0];
    await flush();
    expect(button(panel.root, '切换开发分支').disabled).toBe(false);
    input(panel.root, '.scm-projects label:nth-child(1) input', '789');
    await flush();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
    expect(button(panel.root, '切换开发分支').title).toContain('请先加载当前开发 SCM');
  } finally {
    panel.close();
  }
});
it.each(['合并到测试分支', '切换到测试分支'])(
  '%s 预检后清空测试 ID 立即隐藏测试分支并清除计划，开发加载清空全部旧测试目标',
  async (operation) => {
    const initial = fixture();
    initial.subProjects.push({
      ...initial.subProjects[0],
      id: 'unlinked',
      name: 'Unlinked',
      path: '/example/unlinked',
      scmGitId: '99',
      testBranch: 'test/stale-unlinked',
    });
    const panel = await ready(initial);
    try {
      // 模拟仍带旧测试目标的未关联本地配置，开发范围不包含该仓库。
      panel.project.value = {
        ...panel.project.value,
        subProjects: panel.project.value.subProjects.map((repo) =>
          repo.id === 'unlinked' ? { ...repo, testBranch: 'test/stale-unlinked' } : repo,
        ),
      };
      await flush();
      expect(panel.root.querySelectorAll('.repository')).toHaveLength(1);
      click(panel.root, operation);
      await flush();
      expect(panel.root.querySelector('.plan')).not.toBeNull();
      expect(panel.root.querySelector('[data-test]')?.textContent).toBe('test');
      input(panel.root, '.scm-projects label:nth-child(2) input', '');
      await flush();
      expect(panel.root.querySelector('[data-test]')).toBeNull();
      expect(panel.root.querySelector('.plan')).toBeNull();
      vi.mocked(api.prepareBackendGit).mockClear();
      for (const name of ['切换到测试分支', '合并到测试分支']) {
        expect(button(panel.root, name).disabled).toBe(true);
        click(panel.root, name);
      }
      expect(api.prepareBackendGit).not.toHaveBeenCalled();
      vi.mocked(api.scmProjectModules).mockClear();
      panel.save.mockClear();
      click(panel.root, '加载模块');
      await flush();
      expect(api.scmProjectModules).toHaveBeenCalledExactlyOnceWith('123');
      expect(panel.save).toHaveBeenCalledOnce();
      const saved = panel.save.mock.calls[0][0] as Project;
      expect(saved.scmTestProjectId).toBe('');
      expect(
        saved.subProjects.map((repo) => ({ id: repo.id, testBranch: repo.testBranch })),
      ).toEqual([
        { id: 'repo', testBranch: '' },
        { id: 'unlinked', testBranch: '' },
      ]);
      expect(api.executeBackendGit).not.toHaveBeenCalled();
    } finally {
      panel.close();
    }
  },
);
it('缺失测试匹配清空目标并警告，开发切换仍可预检', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '456';
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '123'
      ? [{ gitId: '42', moduleName: 'service', branch: 'feature/new', gitRemoteUrl: null }]
      : [],
  );
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(panel.root.textContent).toContain('已清空旧测试目标');
    expect(panel.root.querySelector<HTMLElement>('[data-test]')!.textContent).toBe('未配置');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(button(panel.root, '切换开发分支').disabled).toBe(false);
    expect(button(panel.root, '合并到测试分支').disabled).toBe(true);
    expect(button(panel.root, '合并到测试分支').title).toContain('明确且不同');
  } finally {
    panel.close();
  }
});
it('开发 URL 歧义与未匹配模块提示维护路径，保留所有配置且不能预检', async () => {
  const initial = fixture();
  initial.subProjects[0].scmGitId = '';
  vi.mocked(api.scmProjectModules).mockResolvedValue(
    ['42', '43'].map((gitId) => ({
      gitId,
      moduleName: `module-${gitId}`,
      gitRemoteUrl: 'https://example.invalid/team/service.git',
      branch: 'new',
    })),
  );
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(0);
    expect(panel.root.textContent).toContain('核对');
    expect(panel.root.textContent).toContain('添加');
    expect(button(panel.root, '切换开发分支').disabled).toBe(true);
    expect(panel.project.value.subProjects).toEqual(initial.subProjects);
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('已加载范围的新增外部仓库自动匹配并保存，普通状态刷新不重复保存', async () => {
  const panel = await ready();
  try {
    panel.project.value = {
      ...panel.project.value,
      subProjects: [
        ...panel.project.value.subProjects,
        { ...fixture().subProjects[0], id: 'new', name: 'New', developmentBranch: 'old-new' },
      ],
    };
    await flush();
    expect(
      [...panel.root.querySelectorAll<HTMLElement>('[data-development]')].map(
        (node) => node.textContent,
      ),
    ).toEqual(['develop', 'develop']);
    expect(panel.save).toHaveBeenCalledOnce();
    expect(button(panel.root, '切换开发分支').title).toContain('等待保存确认');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    click(panel.root, '刷新仓库');
    await flush();
    expect(panel.root.querySelector<HTMLElement>('[data-development]')!.textContent).toBe(
      'develop',
    );
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith(
      expect.arrayContaining([expect.objectContaining({ id: 'new' })]),
    );
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects).toHaveLength(2);
  } finally {
    panel.close();
  }
});
it('加载使用配置 remote 的最新状态，自动保存失败保留分支并重新加载重试', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '';
  initial.subProjects[0].remote = 'upstream';
  initial.subProjects[0].scmGitId = '';
  vi.mocked(api.backendRepositoryRemoteUrls).mockImplementation(async (repositories) =>
    Object.fromEntries(
      repositories.map((repo) => [
        repo.id,
        repo.remote === 'upstream'
          ? 'https://example.invalid/team/new.git'
          : 'https://example.invalid/team/service.git',
      ]),
    ),
  );
  vi.mocked(api.backendRepositoryStatuses).mockImplementation(async (repositories) =>
    repositories.map((repo) => ({
      id: repo.id,
      currentBranch: 'develop',
      head: 'abc',
      dirty: false,
      merging: false,
      rebasing: false,
      remoteUrl:
        repo.remote === 'upstream'
          ? 'https://example.invalid/team/new.git'
          : 'https://example.invalid/team/service.git',
      branches: [],
      root: repo.path,
      error: '',
    })),
  );
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    {
      gitId: '77',
      moduleName: 'new-service',
      branch: 'feature/latest',
      gitRemoteUrl: 'https://example.invalid/team/new.git',
    },
  ]);
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.backendRepositoryStatuses).toHaveBeenLastCalledWith([
      expect.objectContaining({ remote: 'upstream' }),
    ]);
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      remote: 'upstream',
      scmGitId: '77',
      developmentBranch: 'feature/latest',
    });
    expect(button(panel.root, '切换开发分支').title).toContain('等待保存确认');
    panel.saving.value = true;
    await flush();
    panel.saving.value = false;
    await flush();
    expect(panel.root.textContent).toContain('保存未完成');
    expect(panel.root.querySelector<HTMLElement>('[data-development]')!.textContent).toBe(
      'feature/latest',
    );
    click(panel.root, '切换开发分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).toHaveBeenCalledTimes(2);
    expect(panel.save).toHaveBeenCalledTimes(2);
    expect(panel.save.mock.calls[1][0]).toEqual(panel.save.mock.calls[0][0]);
    panel.project.value = panel.save.mock.calls[1][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
    expect(button(panel.root, '切换开发分支').disabled).toBe(false);
  } finally {
    panel.close();
  }
});
it.each(['dirty', 'merging', 'rebasing', 'error', 'missing'] as const)(
  '仓库状态 %s 阻止切换并显示原因',
  async (condition) => {
    const panel = await ready();
    try {
      vi.mocked(api.backendRepositoryStatuses).mockResolvedValue(
        condition === 'missing'
          ? []
          : [
              {
                id: 'repo',
                currentBranch: 'develop',
                head: 'abc',
                dirty: condition === 'dirty',
                merging: condition === 'merging',
                rebasing: condition === 'rebasing',
                error: condition === 'error' ? '读取失败' : '',
                remoteUrl: '',
                branches: [],
                root: '/example/service',
              },
            ],
      );
      click(panel.root, '刷新仓库');
      await flush();
      expect(button(panel.root, '切换开发分支').disabled).toBe(true);
      expect(button(panel.root, '切换到测试分支').disabled).toBe(true);
      click(panel.root, '切换到测试分支');
      expect(panel.root.querySelector('[data-local-branch]')?.textContent || '').not.toContain(
        '尚未切换',
      );
      expect(button(panel.root, '切换开发分支').title).not.toBe('');
      expect(button(panel.root, '推送开发分支到远程').disabled).toBe(true);
      click(panel.root, '推送开发分支到远程');
      click(panel.root, '切换开发分支');
      expect(api.prepareBackendGit).not.toHaveBeenCalled();
    } finally {
      panel.close();
    }
  },
);
it('首次按唯一 URL 识别 Git ID 后立即自动保存，不被身份刷新锁拦住', async () => {
  const initial = fixture();
  initial.subProjects[0].scmGitId = '';
  const readStatus = vi.mocked(api.backendRepositoryStatuses).getMockImplementation()!;
  vi.mocked(api.backendRepositoryStatuses)
    .mockImplementationOnce(readStatus)
    .mockImplementationOnce(() => new Promise(() => {}));
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    {
      gitId: '42',
      moduleName: 'service',
      gitRemoteUrl: 'https://example.invalid/team/service.git',
      branch: 'feature/first',
    },
  ]);
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      scmGitId: '42',
      developmentBranch: 'feature/first',
    });
  } finally {
    panel.close();
  }
});
it('完整开发 SCM 链接加载并规范保存后，父回执保留关联范围且允许预检', async () => {
  const initial = fixture();
  initial.scmTestProjectId = '';
  initial.scmDevelopmentProjectId = 'https://example.invalid/projects/123';
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'feature/from-link' },
  ]);
  const panel = await mount(initial);
  try {
    click(panel.root, '加载模块');
    await flush();
    expect(api.scmProjectModules).toHaveBeenCalledExactlyOnceWith('123');
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].scmDevelopmentProjectId).toBe('123');
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(1);
    expect(button(panel.root, '切换开发分支').title).toContain('等待保存确认');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(1);
    expect(button(panel.root, '切换开发分支').disabled).toBe(false);
    click(panel.root, '切换开发分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      [expect.objectContaining({ developmentBranch: 'feature/from-link' })],
      'switch',
      false,
      false,
    );
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
function button(root: HTMLElement, text: string): HTMLButtonElement {
  return [...root.querySelectorAll('button')].find((item) => item.textContent?.trim() === text)!;
}

it('没有本地配置的开发模块仍显示未关联卡片和同 Git ID 测试分支', async () => {
  const initial = fixture();
  initial.subProjects = [];
  initial.scmTestProjectId = '456';
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) => [
    {
      gitId: '42',
      moduleName: 'service',
      gitRemoteUrl: null,
      branch: id === '123' ? 'feature/scm-only' : 'test/scm-only',
    },
  ]);
  const panel = await ready(initial);
  try {
    expect(panel.root.textContent).toContain('关联 0 / 配置 0');
    const card = panel.root.querySelector('.repository-unlinked')!;
    expect(card.textContent).toContain('未关联');
    expect(card.querySelector<HTMLElement>('[data-development]')?.textContent).toBe(
      'feature/scm-only',
    );
    expect(card.querySelector<HTMLElement>('[data-test]')?.textContent).toBe('test/scm-only');
    expect(card.querySelector('[type="checkbox"]')).toBeNull();
    click(card as HTMLElement, '关联本地目录');
    expect(panel.edit).toHaveBeenCalledOnce();
    expect(button(panel.root, '切换开发分支').disabled).toBe(true);
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});
it('同一模块匹配多个本地目录时每个仓库只显示一次', async () => {
  const initial = fixture();
  initial.subProjects.push({
    ...initial.subProjects[0],
    id: 'second',
    name: 'Second service',
    path: '/example/second',
  });
  const panel = await ready(initial);
  try {
    expect(panel.root.querySelectorAll('.repository')).toHaveLength(2);
    expect(panel.root.querySelectorAll('[data-local-branch]')).toHaveLength(2);
    expect(
      [...panel.root.querySelectorAll('.repository-title strong')].map((node) => node.textContent),
    ).toEqual(['Service', 'Second service']);
    expect(panel.root.querySelector('.repository-unlinked')).toBeNull();
  } finally {
    panel.close();
  }
});

it.each([
  ['切换开发分支', 'switch', 'develop'],
  ['切换到测试分支', 'switch-test', 'test'],
  ['推送开发分支到远程', 'push-dev', 'origin/develop'],
  ['合并到测试分支', 'merge', 'test'],
] as const)('%s 的确认操作、目标分支与任务标题准确', async (label, operation, target) => {
  const panel = await ready();
  try {
    click(panel.root, label);
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      expect.any(Array),
      operation,
      operation === 'push-dev',
      false,
    );
    const plan = panel.root.querySelector('.plan')!;
    expect(plan.querySelector('p')?.textContent).toContain(label);
    expect(plan.textContent?.includes('执行时重新获取远端')).toBe(operation === 'switch-test');
    expect([...plan.querySelectorAll('.plan-branch code')].at(-1)?.textContent).toBe(target);
    if (operation === 'push-dev') {
      expect(plan.textContent).toContain('本地开发分支：develop');
      expect(plan.querySelector('.plan-branch')?.textContent).toContain('develop');
      expect(plan.textContent).not.toContain('将推送测试分支');
    }
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    expect(panel.task).not.toHaveBeenCalled();
    click(panel.root, '确认执行计划');
    await flush();
    expect(api.executeBackendGit).toHaveBeenCalledExactlyOnceWith('plan');
    expect(panel.task).toHaveBeenCalledExactlyOnceWith('task', label);
  } finally {
    panel.close();
  }
});

it('可以全选和取消全选当前 SCM 列表中的仓库', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'other', name: 'Other' });
  const panel = await ready(initial);
  try {
    const checkboxes = [
      ...panel.root.querySelectorAll<HTMLInputElement>('.repository-title input'),
    ];
    expect(checkboxes).toHaveLength(2);
    expect(button(panel.root, '全选').disabled).toBe(true);

    click(panel.root, '取消全选');
    await flush();
    expect(checkboxes.map((checkbox) => checkbox.checked)).toEqual([false, false]);
    expect(button(panel.root, '取消全选').disabled).toBe(true);
    expect(panel.root.textContent).toContain('已选 0 个仓库');

    click(panel.root, '全选');
    await flush();
    expect(checkboxes.map((checkbox) => checkbox.checked)).toEqual([true, true]);
    expect(panel.root.textContent).toContain('已选 2 个仓库');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('测试切换仅预检选中仓库，即使勾选推送与 pom 策略仍传 false', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'other', name: 'Other' });
  const panel = await ready(initial);
  try {
    const excluded = panel.root.querySelectorAll<HTMLInputElement>('.repository-title input')[1];
    excluded.checked = false;
    excluded.dispatchEvent(new Event('change'));
    for (const label of panel.root.querySelectorAll('.toolbar label.checkbox')) {
      const checkbox = label.querySelector('input')!;
      checkbox.checked = true;
      checkbox.dispatchEvent(new Event('change'));
    }
    await flush();
    click(panel.root, '切换到测试分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledExactlyOnceWith(
      'example',
      [expect.objectContaining({ id: 'repo', testBranch: 'test' })],
      'switch-test',
      false,
      false,
    );
    expect(panel.root.querySelector('.plan')?.textContent).toContain('切换到测试分支');
    expect(panel.root.querySelector('.plan')?.textContent).not.toContain('pom.xml');
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    excluded.checked = true;
    excluded.dispatchEvent(new Event('change'));
    await flush();
    expect(panel.root.querySelector('.plan')).toBeNull();
    for (const checkbox of panel.root.querySelectorAll<HTMLInputElement>(
      '.repository-title input',
    )) {
      checkbox.checked = false;
      checkbox.dispatchEvent(new Event('change'));
      await flush();
    }
    expect(button(panel.root, '切换到测试分支').title).toContain('请至少勾选');
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
  } finally {
    panel.close();
  }
});

it('测试切换允许当前位于其他分支且开发与测试目标相同', async () => {
  const initial = fixture();
  initial.subProjects[0].testBranch = 'develop';
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' },
  ]);
  const readStatus = vi.mocked(api.backendRepositoryStatuses).getMockImplementation()!;
  vi.mocked(api.backendRepositoryStatuses).mockImplementation(async (repositories) =>
    (await readStatus(repositories)).map((status) => ({
      ...status,
      currentBranch: 'feature/local',
    })),
  );
  const panel = await ready(initial);
  try {
    expect(button(panel.root, '合并到测试分支').disabled).toBe(true);
    expect(button(panel.root, '切换到测试分支').disabled).toBe(false);
    click(panel.root, '切换到测试分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      [expect.objectContaining({ testBranch: 'develop' })],
      'switch-test',
      false,
      false,
    );
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('缺少测试分支时禁止测试切换，空目标显示未配置', async () => {
  const initial = fixture();
  initial.subProjects[0].testBranch = '';
  vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
    id === '456'
      ? []
      : [{ gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' }],
  );
  const panel = await ready(initial);
  try {
    expect(panel.root.querySelector('code[data-test]')?.textContent).toBe('未配置');
    expect(button(panel.root, '切换到测试分支').disabled).toBe(true);
    expect(button(panel.root, '切换到测试分支').title).toContain('配置明确的测试分支');
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('测试源草稿与已保存但未加载的测试源均阻止测试切换', async () => {
  const panel = await ready();
  try {
    input(panel.root, '.scm-projects label:nth-child(2) input', '789');
    await flush();
    expect(button(panel.root, '切换到测试分支').title).toContain('请重新加载模块并等待自动保存');
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.project.value = { ...panel.project.value, scmTestProjectId: '789' };
    await flush();
    expect(button(panel.root, '切换开发分支').disabled).toBe(false);
    expect(button(panel.root, '切换到测试分支').title).toContain('请先加载测试目标');
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    vi.mocked(api.scmProjectModules).mockImplementation(async (id) => [
      {
        gitId: '42',
        moduleName: 'service',
        gitRemoteUrl: null,
        branch: id === '123' ? 'develop' : 'test/current',
      },
    ]);
    click(panel.root, '加载模块');
    await flush();
    expect(button(panel.root, '切换到测试分支').title).toContain('等待保存确认');
    panel.project.value = panel.save.mock.calls.at(-1)![0];
    await flush();
    click(panel.root, '切换到测试分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledWith(
      'example',
      [expect.objectContaining({ testBranch: 'test/current' })],
      'switch-test',
      false,
      false,
    );
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it('测试切换在保存和预检锁定期间禁止重复操作', async () => {
  const panel = await ready();
  try {
    panel.saving.value = true;
    await flush();
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.saving.value = false;
    await flush();
    vi.mocked(api.prepareBackendGit).mockReturnValueOnce(new Promise(() => {}));
    click(panel.root, '切换到测试分支');
    await flush();
    expect(button(panel.root, '切换到测试分支').disabled).toBe(true);
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

it.each([false, true])('开发推送仅包含选中可见仓库且不受合并选项 %s 影响', async (checked) => {
  const initial = fixture();
  initial.subProjects.push(
    { ...initial.subProjects[0], id: 'excluded', name: 'Excluded' },
    { ...initial.subProjects[0], id: 'hidden', name: 'Hidden', scmGitId: '99' },
  );
  const panel = await ready(initial);
  try {
    const excluded = panel.root.querySelectorAll<HTMLInputElement>('.repository-title input')[1];
    excluded.checked = false;
    excluded.dispatchEvent(new Event('change'));
    for (const checkbox of panel.root.querySelectorAll<HTMLInputElement>(
      '.toolbar label.checkbox input',
    )) {
      checkbox.checked = checked;
      checkbox.dispatchEvent(new Event('change'));
    }
    await flush();
    const before = JSON.stringify(panel.project.value);
    click(panel.root, '推送开发分支到远程');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenCalledExactlyOnceWith(
      'example',
      [expect.objectContaining({ id: 'repo', developmentBranch: 'develop' })],
      'push-dev',
      true,
      false,
    );
    expect(panel.root.querySelector('.plan')?.textContent).not.toContain('pom.xml');
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    expect(panel.save).not.toHaveBeenCalled();
    expect(JSON.stringify(panel.project.value)).toBe(before);
    excluded.checked = true;
    excluded.dispatchEvent(new Event('change'));
    await flush();
    expect(panel.root.querySelector('.plan')).toBeNull();
    for (const checkbox of panel.root.querySelectorAll<HTMLInputElement>(
      '.repository-title input',
    )) {
      checkbox.checked = false;
      checkbox.dispatchEvent(new Event('change'));
      await flush();
    }
    expect(button(panel.root, '推送开发分支到远程').title).toContain('请至少勾选');
    click(panel.root, '推送开发分支到远程');
    expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
  } finally {
    panel.close();
  }
});

it.each(['feature/local', '', 'test'])(
  '当前分支 %j 不在开发分支时禁止推送且不切分支',
  async (currentBranch) => {
    const readStatus = vi.mocked(api.backendRepositoryStatuses).getMockImplementation()!;
    vi.mocked(api.backendRepositoryStatuses).mockImplementation(async (repositories) =>
      (await readStatus(repositories)).map((status) => ({ ...status, currentBranch })),
    );
    const panel = await ready();
    try {
      expect(button(panel.root, '推送开发分支到远程').disabled).toBe(true);
      expect(button(panel.root, '推送开发分支到远程').title).toContain('请先切换开发分支');
      click(panel.root, '推送开发分支到远程');
      expect(api.prepareBackendGit).not.toHaveBeenCalled();
      expect(panel.save).not.toHaveBeenCalled();
      expect(panel.project.value.subProjects[0].developmentBranch).toBe('develop');
    } finally {
      panel.close();
    }
  },
);

it.each(['absent', 'empty-modules', 'unloaded'] as const)(
  '测试来源 %s 不阻止开发推送',
  async (source) => {
    const initial = fixture();
    initial.subProjects[0].testBranch = '';
    if (source === 'empty-modules') {
      initial.scmTestProjectId = '456';
      vi.mocked(api.scmProjectModules).mockImplementation(async (id) =>
        id === '123'
          ? [{ gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' }]
          : [],
      );
    }
    const panel = await ready(initial);
    try {
      if (source === 'unloaded') {
        panel.project.value = { ...panel.project.value, scmTestProjectId: '456' };
        await flush();
      }
      expect(button(panel.root, '推送开发分支到远程').disabled).toBe(false);
      click(panel.root, '推送开发分支到远程');
      await flush();
      expect(api.prepareBackendGit).toHaveBeenCalledWith(
        'example',
        expect.any(Array),
        'push-dev',
        true,
        false,
      );
      expect(api.executeBackendGit).not.toHaveBeenCalled();
    } finally {
      panel.close();
    }
  },
);

it('开发推送在开发源加载、精确保存、保存及 busy、预检和运行锁下均禁止操作', async () => {
  const panel = await mount();
  try {
    expect(button(panel.root, '推送开发分支到远程').disabled).toBe(true);
    vi.mocked(api.scmProjectModules).mockResolvedValue([
      { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch: 'develop' },
    ]);
    // 首次填充映射必须等父组件精确回执。
    panel.project.value.subProjects[0].scmModuleName = 'old-name';
    await flush();
    click(panel.root, '加载模块');
    await flush();
    expect(button(panel.root, '推送开发分支到远程').title).toContain('等待保存确认');
    click(panel.root, '推送开发分支到远程');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.project.value = panel.save.mock.calls.at(-1)![0];
    await flush();
    for (const lock of [panel.saving, panel.busy]) {
      lock.value = true;
      await flush();
      click(panel.root, '推送开发分支到远程');
      expect(api.prepareBackendGit).not.toHaveBeenCalled();
      lock.value = false;
      await flush();
    }
    const prepare = vi.mocked(api.prepareBackendGit).getMockImplementation()!;
    let release!: (plan: Awaited<ReturnType<typeof api.prepareBackendGit>>) => void;
    vi.mocked(api.prepareBackendGit).mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve;
      }),
    );
    click(panel.root, '推送开发分支到远程');
    await flush();
    click(panel.root, '推送开发分支到远程');
    expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    release(await prepare('example', panel.project.value.subProjects, 'push-dev', true, false));
    await flush();
    click(panel.root, '确认执行计划');
    await flush();
    expect(button(panel.root, '推送开发分支到远程').disabled).toBe(true);
    click(panel.root, '推送开发分支到远程');
    expect(api.prepareBackendGit).toHaveBeenCalledTimes(1);
  } finally {
    panel.close();
  }
});

it('批量操作说明紧邻标题、保留选中数量且仅有未保存配置时显示独立提醒', async () => {
  const panel = await ready();
  try {
    const section = panel.root.querySelector('section[aria-label="Git 批量操作"]')!;
    const heading = section.querySelector('.section-heading')!;
    expect(heading.querySelector('h3')?.textContent).toBe('Git 批量操作');
    expect(heading.querySelector('.git-actions-title .hint')?.textContent).toBe(
      '先获取远端并预检，核对计划后执行。',
    );
    expect(heading.textContent).toContain('已选 1 个仓库');
    expect(section.querySelector('[data-scm-unsaved]')).toBeNull();
    expect(
      [...section.querySelectorAll(':scope > p')].some((node) =>
        node.textContent?.includes('先获取远端并预检，核对计划后执行。'),
      ),
    ).toBe(false);
    input(panel.root, '.scm-projects label:nth-child(2) input', '789');
    await flush();
    expect(section.querySelector('[data-scm-unsaved]')?.textContent).toContain('SCM 配置有更改');
    expect(button(panel.root, '推送开发分支到远程').disabled).toBe(true);
  } finally {
    panel.close();
  }
});

it('长分支在关联与未关联卡片保留完整文本且没有分支输入框', async () => {
  const branch = 'feature/' + 'long-branch/'.repeat(30);
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch },
    { gitId: '43', moduleName: 'unlinked', gitRemoteUrl: null, branch },
  ]);
  const panel = await ready();
  try {
    expect(
      [...panel.root.querySelectorAll('code[data-development]')].map((node) => node.textContent),
    ).toEqual([branch, branch]);
    expect(panel.root.querySelector('input[data-development], input[data-test]')).toBeNull();
    expect(panel.root.textContent).not.toContain('编辑子项目');
  } finally {
    panel.close();
  }
});
