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
    scmDevelopmentProjectId: '123',
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
        onTask: task,
        onNotice: notice,
      }),
  });
  app.mount(root);
  await flush();
  return { project, saving, busy, save, edit, task, notice, root, close: () => app.unmount() };
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
  const button = [...root.querySelectorAll('button')].find(
    (item) => item.textContent?.trim() === text,
  );
  expect(button, text).toBeTruthy();
  button!.click();
}
function input(root: HTMLElement, selector: string, value: string) {
  const field = root.querySelector<HTMLInputElement>(selector)!;
  expect(field.readOnly, selector).toBe(false);
  field.value = value;
  field.dispatchEvent(new Event('input'));
}
it('开发、测试、当前及未关联模块分支支持点击或键盘复制完整名称', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const branch = 'feature/' + 'example-branch_'.repeat(20);
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    { gitId: '42', moduleName: 'service', gitRemoteUrl: null, branch },
    { gitId: '43', moduleName: 'unlinked', gitRemoteUrl: null, branch: 'feature/unlinked' },
  ]);
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

it('推送确认计划中的远端分支复制完整目标，不触发执行', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  const panel = await ready();
  try {
    click(panel.root, '推送开发分支到远程');
    await flush();
    const branches = panel.root.querySelectorAll<HTMLElement>('.plan-branch code');
    branches[0].click();
    await flush();
    expect(writeText).toHaveBeenLastCalledWith('develop');
    branches[branches.length - 1].click();
    await flush();
    expect(writeText).toHaveBeenLastCalledWith('origin/develop');
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

it('切换master不依赖测试目标，预检后确认执行且不采用合并选项', async () => {
  const initial = fixture();
  initial.subProjects[0].testBranch = '';
  const panel = await ready(initial);
  try {
    for (const text of ['合并成功后推送测试分支', 'pom.xml 冲突时保留测试分支版本']) {
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
    const fields = panel.root.querySelectorAll<HTMLInputElement>('[list="backend-scm-projects"]');
    fields[0].focus();
    fields[0].value = '101';
    fields[0].dispatchEvent(
      new InputEvent('input', { inputType: 'insertText', data: '1', bubbles: true }),
    );
    await flush();
    expect(document.activeElement).toBe(fields[0]);
    fields[0].dispatchEvent(new Event('change', { bubbles: true }));
    await flush();
    expect(document.activeElement).not.toBe(fields[0]);
    expect(fields[0].value).toBe('101');
    fields[1].focus();
    fields[1].value = '102';
    fields[1].dispatchEvent(
      new InputEvent('input', { inputType: 'insertReplacementText', data: '102', bubbles: true }),
    );
    await flush();
    expect(document.activeElement).not.toBe(fields[1]);
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
  try {
    click(panel.root, '加载项目列表');
    await flush();
    const options = [
      ...panel.root.querySelectorAll<HTMLOptionElement>('#backend-scm-projects option'),
    ];
    expect(options.map((option) => option.value)).toEqual(projects.map((project) => project.id));
    expect(options.map((option) => option.textContent?.trim())).toEqual(
      projects.map((project) => project.name),
    );
    expect(panel.root.querySelectorAll('[list="backend-scm-projects"]')).toHaveLength(2);
    expect(api.scmProjectModules).not.toHaveBeenCalled();
    expect(api.executeBackendGit).not.toHaveBeenCalled();
  } finally {
    panel.close();
  }
});

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
it('外部新增子项目在开发源加载前不展示，且保留未保存开发 SCM ID', async () => {
  const initial = fixture();
  initial.subProjects = [];
  initial.scmDevelopmentProjectId = '';
  const panel = await mount(initial);
  try {
    input(panel.root, '[list="backend-scm-projects"]', '123');
    await flush();
    panel.project.value = { ...initial, subProjects: fixture().subProjects };
    await flush();
    expect(panel.root.querySelector('.repository-title')).toBeNull();
    expect(panel.root.textContent).toContain('已配置1个子项目，请先加载开发SCM项目');
    expect(panel.root.querySelector<HTMLInputElement>('[list="backend-scm-projects"]')!.value).toBe(
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
it.each(['', 'develop'])('测试分支为 %j 时合并仍被拦截，切换无需测试 SCM ID', async (branch) => {
  const initial = fixture();
  initial.subProjects[0].testBranch = branch;
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

it('pom 冲突策略默认关闭，只有合并预检携带，修改后旧计划失效', async () => {
  const panel = await ready();
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
  vi.resetAllMocks();
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
  vi.mocked(api.scmProjectModules).mockResolvedValue([
    {
      gitId: '42',
      moduleName: 'service',
      gitRemoteUrl: 'https://example.invalid/team/service.git',
      branch: 'develop',
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
          operation === 'switch-master'
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
    for (const selector of ['[data-remote]', '[data-scm-git-id]']) {
      expect(panel.root.querySelector<HTMLInputElement>(selector)!.readOnly, selector).toBe(true);
    }
    expect(panel.root.querySelector('input[data-development], input[data-test]')).toBeNull();
    expect(panel.root.querySelector('code[data-development]')?.textContent).toBe('develop');
    expect(panel.root.querySelector('code[data-test]')?.textContent).toBe('test');
    expect(panel.root.querySelector('[name="password"]')).toBeNull();
    expect(panel.root.querySelector('.connection-summary')?.textContent).toContain('fixture-user');
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
  expect(panel.root.querySelector('.history-toolbar')).toBeNull();
  panel.close();
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
      expect(panel.root.querySelector('.history-toolbar')).toBeNull();
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
    click(panel.root, '编辑项目 / 扫描父目录');
    expect(panel.edit).toHaveBeenCalledOnce();
    panel.edit.mockClear();
    expect(section.querySelector<HTMLElement>('.repository [data-development]')?.textContent).toBe(
      'feature/current',
    );
    expect(section.querySelector<HTMLElement>('.repository [data-test]')?.textContent).toBe(
      'test/current',
    );
    expect(section.querySelectorAll('[data-local-branch]')).toHaveLength(1);
    expect(section.querySelector('[data-local-branch]')?.textContent).toContain('develop');
    expect(section.querySelector('[data-local-branch]')?.textContent).toContain('尚未切换');
    const unlinked = section.querySelector('.repository-unlinked')!;
    expect(unlinked.textContent).toContain('missing-test');
    expect(unlinked.textContent).toContain('未关联');
    expect(unlinked.querySelector('[type="checkbox"]')).toBeNull();
    expect(unlinked.querySelector<HTMLInputElement>('[data-scm-git-id]')?.value).toBe('43');
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
    expect(local()).toContain('尚未切换');
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
    input(panel.root, '.scm-projects label:nth-child(2) input', '456');
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
  initial.subProjects[0].remote = 'upstream';
  initial.subProjects[0].scmGitId = '';
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
    input(panel.root, '.scm-projects label:nth-child(2) input', '456');
    await flush();
    expect(button(panel.root, '切换到测试分支').title).toContain('请重新加载模块并等待自动保存');
    click(panel.root, '切换到测试分支');
    expect(api.prepareBackendGit).not.toHaveBeenCalled();
    panel.project.value = { ...panel.project.value, scmTestProjectId: '456' };
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
    input(panel.root, '.scm-projects label:nth-child(2) input', '456');
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
