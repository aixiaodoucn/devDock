// @vitest-environment happy-dom
import { createApp, h, nextTick } from 'vue';
import { expect, it, vi } from 'vitest';
import ScmCreateProjectForm from './ScmCreateProjectForm.vue';

it('回填编辑字段并保留候选之外的负责人，仅提交不重复的新增模块', async () => {
  const options = {
    users: [{ value: 'dev', label: '开发' }],
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
  const edit = { options, input, authors: ['legacy'], revision: 'r', existingModules: ['1'] };
  const update = vi.fn();
  const submit = vi.fn();
  const root = document.createElement('div');
  const app = createApp({
    render: () =>
      h(ScmCreateProjectForm, {
        options,
        edit,
        projectId: '42',
        disabled: false,
        onUpdate: update,
        onSubmit: submit,
      }),
  });
  app.mount(root);
  try {
    const form = root.querySelector('form')!;
    expect(form.getAttribute('aria-label')).toBe('修改 SCM 项目');
    for (const key of ['name', 'branchSuffix', 'details', 'projectType'] as const) {
      expect(form.querySelector<HTMLInputElement>(`[name=${key}]`)?.value).toBe(input[key]);
    }
    const authors = form.querySelector<HTMLSelectElement>('select[multiple]')!;
    expect(Array.from(authors.selectedOptions, (option) => option.value)).toEqual(['legacy']);
    authors.querySelector<HTMLOptionElement>('option[value=dev]')!.selected = true;
    authors.dispatchEvent(new Event('change'));
    const existing = form.querySelector('[aria-label="已有模块"]')!;
    expect(existing.textContent).toContain('支付');
    expect(existing.querySelector('button')).toBeNull();
    const choices = form.querySelector('[aria-label="选择模块"]')!;
    expect(choices.textContent).not.toContain('支付');
    const add = choices.querySelector<HTMLButtonElement>('button')!;
    add.click();
    add.click();
    await nextTick();
    expect(form.querySelectorAll('[aria-label="已选模块"] button')).toHaveLength(1);
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(update).toHaveBeenCalledTimes(1);
    const [draft, selected] = update.mock.calls[0];
    expect(draft).toMatchObject({ ...input, user: selected[0], modules: ['2'] });
    expect([...selected].sort()).toEqual(['dev', 'legacy']);
    expect(edit.authors).toEqual(['legacy']);
    expect(edit.input.modules).toEqual([]);
    expect(submit).not.toHaveBeenCalled();
    for (const option of authors.options) option.selected = false;
    authors.dispatchEvent(new Event('change'));
    await nextTick();
    expect(form.querySelector<HTMLButtonElement>('button[type=submit]')?.disabled).toBe(true);
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(update).toHaveBeenCalledTimes(1);
  } finally {
    app.unmount();
  }
});
