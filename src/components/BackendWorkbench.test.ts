// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { beforeEach, expect, it, vi } from 'vitest';
import BackendWorkbench from './BackendWorkbench.vue';
import { api } from '../api';
import { listen } from '@tauri-apps/api/event';
import { newProject, type Project } from '../types';
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
    scmStatus: vi.fn(),
    scmProjects: vi.fn(),
    scmProjectModules: vi.fn(),
    backendRepositoryStatuses: vi.fn(),
    prepareBackendGit: vi.fn(),
    executeBackendGit: vi.fn(),
    backendGitState: vi.fn(),
    backendGitOperations: vi.fn(),
    openPath: vi.fn(),
    cancel: vi.fn(),
    abortBackendGit: vi.fn(),
    completeBackendGit: vi.fn(),
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
    subProjects: [
      {
        id: 'repo',
        name: 'Service',
        path: '/example/service',
        remote: 'origin',
        scmGitId: '',
        scmModuleName: '',
        developmentBranch: 'develop',
        testBranch: 'test',
      },
    ],
  };
}
async function mount(initial = fixture()) {
  const project = ref(initial);
  const saving = ref(false);
  const save = vi.fn();
  const root = document.createElement('div');
  const app = createApp({
    render: () =>
      h(BackendWorkbench, {
        project: project.value,
        saving: saving.value,
        busy: false,
        onSave: save,
      }),
  });
  app.mount(root);
  await flush();
  return { project, saving, save, root, close: () => app.unmount() };
}
function click(root: HTMLElement, text: string) {
  const button = [...root.querySelectorAll('button')].find(
    (item) => item.textContent?.trim() === text,
  );
  expect(button, text).toBeTruthy();
  button!.click();
}
function input(root: HTMLElement, selector: string, value: string) {
  const field = root.querySelector<HTMLInputElement>(selector)!;
  field.value = value;
  field.dispatchEvent(new Event('input'));
}
it('仅开发 SCM ID 可加载、应用并保存，保留已有测试分支', async () => {
  const initial = fixture();
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
    click(panel.root, '应用映射并保存');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      developmentBranch: 'feature/new',
      testBranch: 'test',
    });
    expect(panel.save.mock.calls[0][0].scmTestProjectId).toBe('');
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
  } finally {
    panel.close();
  }
});
it('外部新增子项目立即显示且保留未保存开发 SCM ID', async () => {
  const initial = fixture();
  initial.subProjects = [];
  const panel = await mount(initial);
  try {
    input(panel.root, '[list="backend-scm-projects"]', '123');
    await flush();
    panel.project.value = { ...initial, subProjects: fixture().subProjects };
    await flush();
    expect(panel.root.querySelector('.repository-title')?.textContent).toContain('Service');
    expect(panel.root.querySelector<HTMLInputElement>('[list="backend-scm-projects"]')!.value).toBe(
      '123',
    );
    expect(panel.root.querySelector<HTMLInputElement>('.repository-title input')!.checked).toBe(
      true,
    );
    click(panel.root, '保存映射');
    await flush();
    expect(panel.save.mock.calls[0][0]).toMatchObject({
      scmDevelopmentProjectId: '123',
      subProjects: fixture().subProjects,
    });
  } finally {
    panel.close();
  }
});
it('填写非法测试 ID 时加载和保存均显示明确错误，且不请求模块', async () => {
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
    click(panel.root, '保存映射');
    await flush();
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
    click(panel.root, '应用映射并保存');
    await flush();
    expect(panel.save).not.toHaveBeenCalled();
    expect(panel.root.querySelector('[role="alert"]')?.textContent).toContain('测试分支');
  } finally {
    panel.close();
  }
});
it.each(['', 'develop'])('测试分支为 %j 时合并仍被拦截，切换无需测试 SCM ID', async (branch) => {
  const initial = fixture();
  initial.subProjects[0].testBranch = branch;
  const panel = await mount(initial);
  try {
    click(panel.root, '合并到测试分支');
    await flush();
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    expect(panel.root.textContent).toContain('明确且不同的开发分支和测试分支');
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
it('外部增删和重命名以新配置为准，同仓库保留无冲突的草稿字段', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'removed', name: 'Removed' });
  const panel = await mount(initial);
  try {
    input(panel.root, '.scm-projects label:nth-child(1) input', '123');
    input(panel.root, '.scm-projects label:nth-child(2) input', '456');
    input(panel.root, '.repository .mapping-fields label:nth-child(1) input', 'upstream');
    input(panel.root, '[data-development]', 'local-dev');
    input(panel.root, '.repository .mapping-fields label:nth-child(3) input', 'local-test');
    input(panel.root, '.repository .mapping-fields label:nth-child(4) input', '42');
    await flush();
    const updated = fixture();
    updated.scmTestProjectId = '789';
    updated.subProjects[0].name = 'Renamed';
    updated.subProjects[0].developmentBranch = 'external-dev';
    updated.subProjects.push({ ...fixture().subProjects[0], id: 'new', name: 'New' });
    panel.project.value = updated;
    await flush();
    expect(
      [...panel.root.querySelectorAll('.repository-title strong')].map((node) => node.textContent),
    ).toEqual(['Renamed', 'New']);
    expect(
      [...panel.root.querySelectorAll<HTMLInputElement>('.repository-title input')].every(
        (node) => node.checked,
      ),
    ).toBe(true);
    click(panel.root, '保存映射');
    await flush();
    expect(panel.save.mock.calls[0][0]).toMatchObject({
      scmDevelopmentProjectId: '123',
      scmTestProjectId: '789',
      subProjects: [
        {
          id: 'repo',
          name: 'Renamed',
          remote: 'upstream',
          scmGitId: '42',
          developmentBranch: 'external-dev',
          testBranch: 'local-test',
        },
        { id: 'new', name: 'New' },
      ],
    });
    panel.project.value = panel.save.mock.calls[0][0];
    await flush();
    expect(panel.root.textContent).toContain('映射已保存');
    expect(panel.root.textContent).not.toContain('等待保存确认');
  } finally {
    panel.close();
  }
});
it.each(['path', 'remote'] as const)('外部改变仓库 %s 后不保留旧仓库的草稿映射', async (field) => {
  const panel = await mount();
  try {
    input(panel.root, '.scm-projects label:nth-child(1) input', '123');
    input(panel.root, '.mapping-fields label:nth-child(1) input', 'local-remote');
    input(panel.root, '[data-development]', 'local-dev');
    input(panel.root, '.mapping-fields label:nth-child(3) input', 'local-test');
    input(panel.root, '.mapping-fields label:nth-child(4) input', 'old-git');
    await flush();
    const updated = fixture();
    updated.subProjects[0][field] = field === 'path' ? '/example/another' : 'external-remote';
    panel.project.value = updated;
    await flush();
    click(panel.root, '保存映射');
    await flush();
    expect(panel.save.mock.calls[0][0].subProjects).toEqual(updated.subProjects);
    expect(panel.save.mock.calls[0][0].scmDevelopmentProjectId).toBe('123');
  } finally {
    panel.close();
  }
});
it('连续外部更新以独立快照合并，原地变更也能保留草稿并移除失效计划', async () => {
  const panel = await mount();
  try {
    click(panel.root, '切换开发分支');
    await flush();
    expect(panel.root.textContent).toContain('确认执行计划');
    panel.project.value.subProjects[0].name = 'Renamed';
    await flush();
    expect(panel.root.textContent).not.toContain('确认执行计划');
    input(panel.root, '[data-development]', 'local-dev');
    await flush();
    panel.project.value.subProjects.push({ ...fixture().subProjects[0], id: 'new', name: 'New' });
    await flush();
    panel.project.value.subProjects[0].testBranch = 'external-test';
    await flush();
    click(panel.root, '保存映射');
    await flush();
    expect(panel.save.mock.calls[0][0].subProjects[0]).toMatchObject({
      name: 'Renamed',
      developmentBranch: 'local-dev',
      testBranch: 'external-test',
    });
    expect(panel.save.mock.calls[0][0].subProjects).toHaveLength(2);
  } finally {
    panel.close();
  }
});
it('新增仓库加入选择、删除仓库剔除，保留取消选择且排除错误目录', async () => {
  const initial = fixture();
  initial.subProjects.push({ ...initial.subProjects[0], id: 'removed', name: 'Removed' });
  const panel = await mount(initial);
  try {
    const checkbox = panel.root.querySelector<HTMLInputElement>('.repository-title input')!;
    checkbox.checked = false;
    checkbox.dispatchEvent(new Event('change'));
    await flush();
    vi.mocked(api.backendRepositoryStatuses).mockResolvedValue([
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
    expect([...checkboxes].map((node) => node.checked)).toEqual([false, true, false]);
    expect(checkboxes[2].disabled).toBe(true);
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
  const panel = await mount();
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
it('外部结构更新不冒充保存回执，保存失败可继续编辑且新仓库仍显示', async () => {
  const panel = await mount();
  try {
    input(panel.root, '[data-development]', 'local-dev');
    await flush();
    click(panel.root, '保存映射');
    await flush();
    panel.saving.value = true;
    await flush();
    panel.project.value = {
      ...fixture(),
      subProjects: [
        ...fixture().subProjects,
        { ...fixture().subProjects[0], id: 'new', name: 'New' },
      ],
    };
    await flush();
    expect(panel.root.textContent).toContain('等待保存确认');
    expect(panel.root.textContent).not.toContain('映射已保存');
    panel.saving.value = false;
    await flush();
    expect(panel.root.textContent).toContain('保存未完成');
    expect(panel.root.querySelector<HTMLInputElement>('[data-development]')!.value).toBe(
      'local-dev',
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
    const checkboxes = panel.root.querySelectorAll<HTMLInputElement>('.repository-title input');
    expect(checkboxes[1].checked).toBe(false);
    expect(checkboxes[1].disabled).toBe(true);
    click(panel.root, '加载模块');
    await flush();
    click(panel.root, '应用映射并保存');
    await flush();
    expect(panel.save).toHaveBeenCalledOnce();
    const saved = panel.save.mock.calls[0][0] as Project;
    expect(saved.subProjects[0].developmentBranch).toBe('feature/new');
    expect(saved.subProjects[1]).toEqual(directory);
  } finally {
    panel.close();
  }
});

it('pom 冲突策略默认关闭，只有合并预检携带，修改后旧计划失效', async () => {
  const panel = await mount();
  try {
    const checkbox = panel.root.querySelector<HTMLInputElement>('[data-keep-test-pom]');
    expect(checkbox).not.toBeNull();
    const toggle = async (checked: boolean) => {
      checkbox!.checked = checked;
      checkbox!.dispatchEvent(new Event('change', { bubbles: true }));
      await flush();
    };
    expect(checkbox!.checked).toBe(false);
    click(panel.root, '合并到测试分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenLastCalledWith(
      'example',
      expect.any(Array),
      'merge',
      false,
      false,
    );
    expect(panel.root.querySelector('.plan')?.textContent).toContain('暂停并手动处理');
    await toggle(true);
    expect(panel.root.querySelector('.plan')).toBeNull();
    click(panel.root, '合并到测试分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenLastCalledWith(
      'example',
      expect.any(Array),
      'merge',
      false,
      true,
    );
    expect(panel.root.querySelector('.plan')?.textContent).toContain('保留测试分支版本');
    expect(api.executeBackendGit).not.toHaveBeenCalled();
    await toggle(false);
    expect(panel.root.querySelector('.plan')).toBeNull();
    await toggle(true);
    click(panel.root, '切换开发分支');
    await flush();
    expect(api.prepareBackendGit).toHaveBeenLastCalledWith(
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

beforeEach(() => {
  vi.clearAllMocks();
  env.desktop = true;
  vi.mocked(listen).mockImplementation(async (_event, handler) => {
    env.handler = handler as typeof env.handler;
    return env.unlisten;
  });
  vi.mocked(api.scmStatus).mockResolvedValue({
    connected: false,
    baseUrl: null,
    username: null,
    remembered: false,
    message: null,
  });
  vi.mocked(api.backendRepositoryStatuses).mockResolvedValue([]);
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
        currentBranch: 'main',
        sourceSha: 'abc123',
        targetBranch: repository.developmentBranch,
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
  const panel = await mount();
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
  expect((panel.root.querySelector('[data-save]') as HTMLButtonElement).disabled).toBe(true);
  panel.close();
});
it('未保存映射阻止预检，保存 payload 无凭据且等父回执', async () => {
  const panel = await mount();
  const input = panel.root.querySelector('[data-development]') as HTMLInputElement;
  input.value = 'feature/new';
  input.dispatchEvent(new Event('input'));
  const credential = panel.root.querySelector('[name="password"]') as HTMLInputElement;
  credential.value = 'fixture-secret';
  credential.dispatchEvent(new Event('input'));
  await flush();
  click(panel.root, '保存映射');
  await flush();
  expect(panel.save).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(panel.save.mock.calls)).not.toMatch(
    /password|session|username|fixture-secret/,
  );
  expect(api.prepareBackendGit).not.toHaveBeenCalled();
  expect(panel.root.textContent).toContain('等待保存确认');
  panel.project.value = panel.save.mock.calls[0][0];
  await flush();
  expect(panel.root.textContent).toContain('映射已保存');
  panel.close();
});
it('手动保存将 SCM 链接规范成 ID，按规范快照确认，非法链接阻止保存', async () => {
  const panel = await mount();
  const inputs = panel.root.querySelectorAll<HTMLInputElement>('[list="backend-scm-projects"]');
  inputs[0].value = 'https://example.invalid/projects/123';
  inputs[0].dispatchEvent(new Event('input'));
  inputs[1].value = ' 456 ';
  inputs[1].dispatchEvent(new Event('input'));
  await flush();
  click(panel.root, '保存映射');
  await flush();
  expect(panel.save.mock.calls[0][0]).toMatchObject({
    scmDevelopmentProjectId: '123',
    scmTestProjectId: '456',
  });
  panel.project.value = panel.save.mock.calls[0][0];
  await flush();
  expect(panel.root.textContent).toContain('映射已保存');
  inputs[0].value = 'https://example.invalid/projects/no-id';
  inputs[0].dispatchEvent(new Event('input'));
  await flush();
  click(panel.root, '保存映射');
  await flush();
  expect(panel.save).toHaveBeenCalledTimes(1);
  expect(panel.root.textContent).toContain('请输入纯数字 SCM 项目 ID');
  expect(panel.root.textContent).not.toContain('等待保存确认');
  panel.close();
});
it('预览不调用 native 和事件监听', async () => {
  env.desktop = false;
  const panel = await mount();
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
  const panel = await mount();
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
  const panel = await mount();
  panel.close();
  register(env.unlisten);
  await flush();
  expect(env.unlisten).toHaveBeenCalledTimes(1);
});
it('事件乱序响应不会把成功覆盖为旧的运行状态', async () => {
  const panel = await mount();
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
  expect(panel.root.textContent).toContain('操作状态 · success');
  panel.close();
});
it('保存中与预检中均禁止重复提交操作，合并默认不推送', async () => {
  const panel = await mount();
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
  const panel = await mount();
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
