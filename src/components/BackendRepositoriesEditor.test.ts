// @vitest-environment happy-dom
import { createApp, h, nextTick, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import BackendRepositoriesEditor, {
  backendRepositoriesError,
} from './BackendRepositoriesEditor.vue';
import { api } from '../api';
import type { BackendCandidate, BackendRepository } from '../types';
vi.mock('../api', () => ({
  desktop: true,
  api: { scanBackendChildren: vi.fn(), gitBranch: vi.fn(), choosePath: vi.fn() },
}));
const candidate = (name = 'service', isGit = true): BackendCandidate => ({
  id: name,
  name,
  path: `/parent/${name}`,
  isGit,
  currentBranch: 'dev',
  error: '',
});
async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await nextTick();
}
function mountEditor(saved: BackendRepository[] = []) {
  const parent = ref('/parent');
  const repos = ref(saved);
  const root = document.createElement('div');
  document.body.append(root);
  const component = ref<InstanceType<typeof BackendRepositoriesEditor> | null>(null);
  const app = createApp({
    render: () =>
      h(BackendRepositoriesEditor, {
        ref: component,
        parentPath: parent.value,
        modelValue: repos.value,
        'onUpdate:modelValue': (value: BackendRepository[]) => {
          repos.value = value;
        },
      }),
  });
  app.mount(root);
  return {
    root,
    parent,
    repos,
    collect: () => component.value!.collectRepositories(),
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
function click(root: HTMLElement, text: string) {
  Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
    .find((b) => b.textContent?.includes(text))!
    .click();
}
async function input(root: HTMLElement, label: string, value: string) {
  const el = root.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;
  el.value = value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  await nextTick();
}
describe('后端子项目编辑', () => {
  it('保存校验拒绝空ID、相对路径、重复路径及所有元数据中的NUL', () => {
    const repository: BackendRepository = {
      id: 'r',
      name: '服务',
      path: 'C:/repo',
      remote: 'origin',
      scmGitId: '',
      scmModuleName: '',
      developmentBranch: '',
      testBranch: '',
    };
    expect(backendRepositoriesError([repository])).toBe('');
    for (const field of Object.keys(repository) as (keyof BackendRepository)[]) {
      expect(backendRepositoriesError([{ ...repository, [field]: 'bad\0value' }])).toContain('NUL');
    }
    expect(backendRepositoriesError([{ ...repository, id: ' ' }])).toContain('ID');
    expect(backendRepositoriesError([{ ...repository, path: 'relative' }])).toContain('绝对路径');
    expect(
      backendRepositoriesError([repository, { ...repository, id: 'other', path: 'c:\\REPO\\' }]),
    ).toContain('目录重复');
    expect(backendRepositoriesError([repository, { ...repository, path: 'C:/other' }])).toContain(
      'ID 重复',
    );
  });
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.scanBackendChildren).mockResolvedValue([candidate(), candidate('docs', false)]);
    vi.mocked(api.gitBranch).mockResolvedValue('dev');
  });
  it('扫描仅提供候选，Git默认选中，普通目录可确认添加；手动修改名称路径后保存元数据', async () => {
    const editor = mountEditor();
    try {
      await flush();
      expect(api.scanBackendChildren).toHaveBeenCalledWith('/parent');
      expect(editor.repos.value).toEqual([]);
      const checks = editor.root.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
      expect(checks[0].checked).toBe(true);
      expect(checks[1].checked).toBe(false);
      await input(editor.root, '候选 1 名称', 'renamed');
      await input(editor.root, '候选 1 目录', '/elsewhere/service');
      checks[1].click();
      await nextTick();
      expect(editor.collect()).toEqual([
        expect.objectContaining({ name: 'renamed', path: '/elsewhere/service', remote: 'origin' }),
        expect.objectContaining({ name: 'docs' }),
      ]);
      expect(editor.repos.value).toEqual([]);
      expect(editor.root.textContent).not.toContain('添加选中子项目');
      editor.repos.value = editor.collect();
      await flush();
      await input(editor.root, '子项目 1 名称', 'manual');
      await input(editor.root, '子项目 1 目录', '/manual');
      expect(editor.repos.value[0]).toMatchObject({
        name: 'manual',
        path: '/manual',
        remote: 'origin',
      });
      expect(api.gitBranch).toHaveBeenCalledWith('/manual');
      expect(editor.root.textContent).not.toContain('运行项目');
    } finally {
      editor.dispose();
    }
  });
  it('重复扫描按pathKey去重并保留已有名称映射，不复制候选中的凭据', async () => {
    const saved = {
      id: 'saved',
      name: 'custom',
      path: 'C:/REPO',
      remote: 'upstream',
      scmGitId: '7',
      scmModuleName: 'module',
      developmentBranch: 'dev',
      testBranch: 'test',
    };
    vi.mocked(api.scanBackendChildren).mockResolvedValue([
      {
        ...candidate(),
        path: 'c:\\repo\\',
        password: 'secret',
        cookie: 'session-secret',
      } as BackendCandidate,
    ]);
    const editor = mountEditor([saved]);
    try {
      await flush();
      expect(editor.collect()).toEqual([saved]);
      expect(editor.root.querySelectorAll('.candidate-card')).toHaveLength(0);
      click(editor.root, '重新扫描');
      await flush();
      expect(editor.collect()).toEqual([saved]);
      expect(editor.repos.value).toEqual([saved]);
      expect(editor.root.textContent).not.toContain('secret');
    } finally {
      editor.dispose();
    }
  });
  it('父目录变化清空候选，忽略旧请求且保留已保存子项目', async () => {
    let resolveOld!: (value: BackendCandidate[]) => void;
    vi.mocked(api.scanBackendChildren)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveOld = resolve;
          }),
      )
      .mockResolvedValueOnce([candidate('new')]);
    const editor = mountEditor();
    try {
      editor.parent.value = '/new';
      await flush();
      expect(api.scanBackendChildren).toHaveBeenLastCalledWith('/new');
      resolveOld([candidate('stale')]);
      await flush();
      expect(editor.root.textContent).not.toContain('stale');
      expect(editor.root.querySelector<HTMLInputElement>('[aria-label="候选 1 名称"]')?.value).toBe(
        'new',
      );
      editor.repos.value = editor.collect();
      await flush();
      editor.parent.value = '';
      await flush();
      expect(editor.root.querySelector('[aria-label="候选 1 名称"]')).toBeNull();
      expect(editor.repos.value).toHaveLength(1);
    } finally {
      editor.dispose();
    }
  });
});
