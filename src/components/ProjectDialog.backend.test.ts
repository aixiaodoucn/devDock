// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import { api } from '../api';
import { newProject } from '../types';
vi.mock('../api', () => ({
  desktop: true,
  api: { scanBackendChildren: vi.fn(), gitBranch: vi.fn(), nodeVersions: vi.fn() },
}));
async function mountDialog() {
  const save = vi.fn();
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(ProjectDialog, {
        project: {
          ...newProject(),
          type: 'backend',
          name: '后端',
          path: '/parent',
          platforms: [],
          subProjects: [
            {
              id: 'r',
              name: '服务',
              path: '/repo',
              remote: 'origin',
              scmGitId: '',
              scmModuleName: '',
              developmentBranch: '',
              testBranch: '',
            },
          ],
        },
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
    async submit() {
      root
        .querySelector('form')!
        .dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await nextTick();
    },
    dispose() {
      app.unmount();
      root.remove();
    },
  };
}
async function input(root: HTMLElement, label: string, value: string) {
  const el = root.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;
  el.value = value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  await nextTick();
}
describe('后端项目保存', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.scanBackendChildren).mockResolvedValue([]);
    vi.mocked(api.gitBranch).mockResolvedValue('dev');
    vi.mocked(api.nodeVersions).mockResolvedValue([]);
  });
  it('显示元数据和当前分支，拒绝无效子项目及SCM项目ID', async () => {
    const dialog = await mountDialog();
    try {
      expect(dialog.root.textContent).not.toContain('预留');
      expect(dialog.root.querySelector('[aria-label="当前 Git 分支"]')).not.toBeNull();
      await input(dialog.root, '子项目 1 名称', '');
      await dialog.submit();
      expect(dialog.save).not.toHaveBeenCalled();
      expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain('子项目');
      await input(dialog.root, '子项目 1 名称', '服务');
      await input(dialog.root, 'SCM 开发项目 ID', 'abc');
      await dialog.submit();
      expect(dialog.save).not.toHaveBeenCalled();
      expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain('纯数字');
      await input(dialog.root, 'SCM 开发项目 ID', '123');
      await dialog.submit();
      expect(dialog.save).toHaveBeenCalledWith(
        expect.objectContaining({
          scmDevelopmentProjectId: '123',
          subProjects: [expect.objectContaining({ name: '服务' })],
        }),
      );
    } finally {
      dialog.dispose();
    }
  });
  it('切类型保留配置并阻止非backend保存，切回后端配置仍在', async () => {
    const dialog = await mountDialog();
    try {
      const select = dialog.root.querySelector<HTMLSelectElement>('#project-type')!;
      select.value = 'nodejs';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
      await dialog.submit();
      expect(dialog.save).not.toHaveBeenCalled();
      expect(dialog.root.querySelector('[role="alert"]')?.textContent).toContain('切回后端');
      select.value = 'backend';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      await nextTick();
      expect(
        dialog.root.querySelector<HTMLInputElement>('[aria-label="子项目 1 名称"]')?.value,
      ).toBe('服务');
    } finally {
      dialog.dispose();
    }
  });
});
