// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PathInput from './PathInput.vue';
import { api } from '../api';

vi.mock('../api', () => ({ api: { choosePath: vi.fn().mockResolvedValue(null) } }));

async function clickPicker(props: { directory?: boolean }) {
  const root = document.createElement('div');
  document.body.append(root);
  // 实际挂载组件，覆盖 Vue 对省略的布尔属性的转换行为。
  const app = createApp({ render: () => h(PathInput, { modelValue: '', ...props }) });
  app.mount(root);
  root.querySelector('button')!.click();
  await nextTick();
  app.unmount();
  root.remove();
}
describe('路径选择器', () => {
  beforeEach(() => vi.clearAllMocks());
  it('省略 directory 属性时选择文件夹', async () => {
    await clickPicker({});
    expect(api.choosePath).toHaveBeenCalledWith(true);
  });
  it('manifest 显式传入 false 时选择文件', async () => {
    await clickPicker({ directory: false });
    expect(api.choosePath).toHaveBeenCalledWith(false);
  });
});
