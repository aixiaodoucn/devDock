// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import SqlpadApproval from './SqlpadApproval.vue';
import ScmProjectApproval from './ScmProjectApproval.vue';

const main = vi.hoisted(() => ({ label: 'main', listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWebviewWindow: () => main }));
const event = 'scm-project-approval-requested';
const list = 'list_scm_project_approvals';
const resolve = 'resolve_scm_project_approval';
const request = () => ({
  requestId: 'first',
  site: 'https://scm.example',
  account: 'operator',
  tool: 'scm_update_project',
  expiresAt: Date.now() + 5000,
  arguments: {
    projectId: 'p-01',
    authors: ['author1', 'author2'],
    revision: 'base-revision',
    input: {
      name: '<project>',
      user: 'owner',
      details: '说明正文',
      projectType: 'type1',
      branchSuffix: 'branch1',
      modules: ['module1', 'module2'],
    },
  },
});
const root = document.createElement('div');
const stop = vi.fn();
let app: ReturnType<typeof createApp>;
let notify: () => Promise<void>;
async function settle() {
  for (let i = 0; i < 6; i++) await nextTick();
}
async function mount() {
  document.body.append(root);
  app = createApp(ScmProjectApproval);
  app.mount(root);
  await settle();
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  main.label = 'main';
  main.listen.mockImplementation(async (_, cb) => {
    notify = cb;
    return stop;
  });
  vi.mocked(invoke).mockResolvedValue([request()]);
});
afterEach(() => {
  app.unmount();
  root.remove();
  vi.useRealTimers();
});
it.each([true, false])(
  '完整展示原始写入，只有显式点击才决定且不重复提交 approved=%s',
  async (approved) => {
    await mount();
    expect(main.listen).toHaveBeenCalledWith(event, expect.any(Function));
    expect(invoke).toHaveBeenCalledExactlyOnceWith(list, undefined);
    const text = root.textContent;
    for (const value of [
      '修改 SCM 项目',
      'https://scm.example',
      'operator',
      'p-01',
      '<project>',
      'owner',
      'module1',
      'module2',
      'author1',
      'author2',
      'branch1',
      'type1',
      '说明正文',
      'base-revision',
    ])
      expect(text).toContain(value);
    expect(root.querySelector('project')).toBeNull();
    root.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true }));
    expect(invoke).toHaveBeenCalledOnce();
    vi.mocked(invoke).mockClear().mockResolvedValueOnce(undefined).mockResolvedValueOnce([]);
    const button = root.querySelectorAll('button')[approved ? 1 : 0];
    button.click();
    button.click();
    await settle();
    expect(vi.mocked(invoke).mock.calls).toEqual([
      [resolve, { requestId: 'first', approved }],
      [list, undefined],
    ]);
    expect(root.querySelector('dialog')).toBeNull();
  },
);
it('按宿主时间倒计时，过期切换到下一项，不提交决定', async () => {
  vi.mocked(invoke).mockResolvedValue([
    request(),
    {
      ...request(),
      tool: 'backend_git_execute',
      requestId: 'second',
      expiresAt: Date.now() + 10000,
      arguments: {
        projectName: 'local-project',
        operation: 'switch-dev',
        push: true,
        repositories: [
          { repository: { name: 'repo-one' }, currentBranch: 'old-main', targetBranch: 'new-dev' },
          { repository: { name: 'repo-two' }, targetBranch: 'dev-two' },
        ],
      },
    },
  ]);
  await mount();
  expect(root.textContent).toContain('5 秒后过期');
  await vi.advanceTimersByTimeAsync(1000);
  expect(root.textContent).toContain('4 秒后过期');
  await vi.advanceTimersByTimeAsync(4000);
  for (const value of [
    '执行 Git 计划',
    'local-project',
    'switch-dev',
    '是否推送',
    '是',
    'repo-one',
    'old-main',
    'new-dev',
    'repo-two',
    'dev-two',
  ])
    expect(root.textContent).toContain(value);
  await vi.advanceTimersByTimeAsync(5000);
  expect(root.querySelector('dialog')).toBeNull();
  expect(invoke).toHaveBeenCalledOnce();
});
it('通知后的新快照优先，恢复队列不复活已撤回请求', async () => {
  let finish!: (v: unknown) => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise((done) => (finish = done)));
  await mount();
  vi.mocked(invoke).mockResolvedValue([]);
  await notify();
  finish([request()]);
  await settle();
  expect(root.querySelector('dialog')).toBeNull();
  vi.mocked(invoke).mockResolvedValue([request()]);
  await notify();
  await settle();
  expect(root.textContent).toContain('p-01');
  app.unmount();
  expect(stop).toHaveBeenCalledOnce();
  await mount();
  expect(root.textContent).toContain('p-01');
});
it('非主窗口不订阅不读取，更不会处理 SQLPad 审批', async () => {
  main.label = 'settings';
  await mount();
  expect(main.listen).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
});

it('解除关联明确显示项目、模块和不删除的边界', async () => {
  vi.mocked(invoke).mockResolvedValue([
    {
      ...request(),
      tool: 'scm_remove_project_module',
      arguments: { projectId: 'project-a', moduleId: 'module-b', revision: 'version-c' },
    },
  ]);
  await mount();
  expect(root.textContent).toMatch(
    /解除项目模块关联.*不删除项目或仓库.*目标项目 ID.*project-a.*目标模块 ID.*module-b.*version-c/s,
  );
});

it('同时到达的 SQLPad 与 SCM 请求使用独立审批通道', async () => {
  const sql = {
    requestId: 'sql',
    connection: 'db',
    preview: 'query',
    expiresAt: Date.now() + 60000,
  };
  vi.mocked(invoke).mockResolvedValueOnce([request()]).mockResolvedValueOnce(sql);
  document.body.append(root);
  app = createApp({ render: () => h('main', [h(ScmProjectApproval), h(SqlpadApproval)]) });
  app.mount(root);
  await settle();
  expect(root.querySelectorAll('dialog')).toHaveLength(2);
  vi.mocked(invoke).mockClear().mockResolvedValueOnce(undefined).mockResolvedValueOnce([]);
  root
    .querySelector<HTMLButtonElement>('dialog[aria-labelledby="scm-approval-title"] button')!
    .click();
  await settle();
  expect(vi.mocked(invoke).mock.calls).toEqual([
    [resolve, { requestId: 'first', approved: false }],
    [list, undefined],
  ]);
  expect(root.querySelector('dialog')?.textContent).toContain('批准本次生产查询');
});
