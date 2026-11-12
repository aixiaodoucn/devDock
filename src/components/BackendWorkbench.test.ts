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
    async (projectId, repositories, operation, push) => ({
      planId: 'plan',
      projectId,
      operation,
      push: !!push,
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
  expect(api.prepareBackendGit).toHaveBeenCalledWith('example', expect.any(Array), 'switch', false);
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
