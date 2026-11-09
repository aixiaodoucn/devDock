// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectDialog from './ProjectDialog.vue';
import { api } from '../api';
import { newProject, type Project } from '../types';

vi.mock('../api', () => ({ desktop: true, api: { nodeVersions: vi.fn(), choosePath: vi.fn() } }));
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
  const root = document.createElement('div');
  document.body.append(root);
  const app = createApp({
    render: () =>
      h(ProjectDialog, {
        project,
        tools: { hbuilderxPath: '', terminals: [], jdks: [] },
        hbuilderxPath: '',
        existing: [],
        saving: false,
        detecting: false,
        onSave: (value: Project) => {
          saved = value;
        },
      }),
  });
  app.mount(root);
  await Promise.resolve();
  await nextTick();
  return {
    root,
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
    platforms: ['pc'],
  };
}
describe('Node 项目版本选择', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.nodeVersions).mockResolvedValue(versions);
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
