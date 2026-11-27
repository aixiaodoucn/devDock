// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { afterEach, expect, it, vi } from 'vitest';
import ScmCreateProjectForm from './ScmCreateProjectForm.vue';

const options = {
  users: [
    { value: 'dev', label: '开发' },
    { value: 'qa', label: '测试' },
  ],
  types: [{ value: 'feature', label: '需求' }],
  modules: [
    { value: '1', label: '支付' },
    { value: '2', label: '仓库' },
  ],
  defaultUser: 'dev',
};
const input = {
  name: '原项目',
  branchSuffix: 'v1',
  details: '说明',
  projectType: 'feature',
  user: 'legacy',
  modules: [],
};
const editData = { options, input, authors: ['legacy'], revision: 'r', existingModules: ['1'] };
let cleanup: () => void;
afterEach(() => cleanup());
function mount(edit = false, disabled = false) {
  const root = document.createElement('div');
  document.body.append(root);
  const submit = vi.fn(),
    update = vi.fn(),
    cancel = vi.fn(),
    remove = vi.fn();
  const app = createApp({
    render: () =>
      h(ScmCreateProjectForm, {
        options,
        disabled,
        edit: edit ? editData : undefined,
        projectId: '42',
        onSubmit: submit,
        onUpdate: update,
        onCancel: cancel,
        onRemoveModule: remove,
      }),
  });
  app.mount(root);
  cleanup = () => {
    app.unmount();
    root.remove();
  };
  const get = <T extends HTMLElement = HTMLElement>(selector: string) =>
    root.querySelector<T>(selector)!;
  const enter = async (name: string, value: string) => {
    const field = get<HTMLInputElement>(`[name=${name}]`);
    field.value = value;
    field.dispatchEvent(new Event('input'));
    await nextTick();
  };
  const save = () => get('form').dispatchEvent(new Event('submit', { cancelable: true }));
  return { root, get, enter, save, submit, update, remove, cancel };
}
it('创建弹框中按名称或账号搜索并多选负责人，提交全部账号', async () => {
  const { get, enter, save, submit, cancel } = mount();
  expect(get<HTMLDialogElement>('dialog').open).toBe(true);
  expect(get('[aria-label="移除负责人 dev"]')).toBeTruthy();
  expect(
    get('input[name=details]')
      .closest('label')
      ?.previousElementSibling?.querySelector('[name=projectType]'),
  ).toBeTruthy();
  expect(get('[name=authorSearch]')).toBeNull();
  get('.author-trigger').click();
  await nextTick();
  await enter('name', '项目');
  for (const query of ['测', 'QA']) {
    await enter('authorSearch', query);
    expect(get('[aria-label="候选负责人"]').querySelectorAll('button')).toHaveLength(1);
    expect(get('[value=qa]')).toBeTruthy();
  }
  get('[value=qa]').click();
  await nextTick();
  save();
  expect(submit).toHaveBeenCalledWith(expect.objectContaining({ user: 'dev' }), ['dev', 'qa']);
  get('dialog').dispatchEvent(new Event('cancel', { cancelable: true }));
  expect(cancel).toHaveBeenCalledOnce();
});
it('编辑时保留历史账号，支持移除和重新选择，只提交新增模块', async () => {
  const { get, enter, save, update } = mount(true);
  for (const key of ['name', 'branchSuffix', 'details', 'projectType'] as const)
    expect(get<HTMLInputElement>(`[name=${key}]`).value).toBe(input[key]);
  expect(get('[aria-label="移除负责人 legacy"]')).toBeTruthy();
  get('[aria-label="移除负责人 legacy"]').click();
  await nextTick();
  expect(get<HTMLButtonElement>('[type=submit]').disabled).toBe(true);
  get('.author-trigger').click();
  await nextTick();
  get('[value=legacy]').click();
  await nextTick();
  get('.author-trigger').click();
  await nextTick();
  get('[value=dev]').click();
  const choices = get('[aria-label="选择模块"]');
  expect(choices.textContent).not.toContain('支付');
  choices.querySelector('button')!.click();
  choices.querySelector('button')!.click();
  await nextTick();
  save();
  expect(update).toHaveBeenCalledExactlyOnceWith({ ...input, modules: ['2'] }, ['legacy', 'dev']);
  expect(editData.authors).toEqual(['legacy']);
});
it('已有模块仅在二次确认后立即发出解除关联事件，取消保留草稿', async () => {
  const { get, enter, remove, cancel, update } = mount(true);
  await enter('name', '草稿');
  const request = get('[aria-label="已有模块"] button');
  request.click();
  await nextTick();
  const confirm = get<HTMLDialogElement>('[role=alertdialog]');
  expect(confirm.open).toBe(true);
  expect(confirm.textContent).toMatch(/立即解除.*不会删除.*仓库/s);
  confirm.querySelector<HTMLButtonElement>('[data-action=cancel]')!.click();
  expect(remove).not.toHaveBeenCalled();
  expect(cancel).not.toHaveBeenCalled();
  request.click();
  await nextTick();
  get('[role=alertdialog] [data-action=confirm]').click();
  expect(remove).toHaveBeenCalledExactlyOnceWith('1');
  expect(update).not.toHaveBeenCalled();
  expect(get<HTMLInputElement>('[name=name]').value).toBe('草稿');
});
it('请求处理中按 Escape 不会关闭表单', () => {
  const { get, cancel } = mount(true, true);
  get('dialog').dispatchEvent(new Event('cancel', { cancelable: true }));
  expect(cancel).not.toHaveBeenCalled();
});
