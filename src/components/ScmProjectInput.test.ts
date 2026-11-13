// @vitest-environment happy-dom
import { createApp, h, nextTick, reactive } from 'vue';
import { afterEach, expect, it, vi } from 'vitest';
import ScmProjectInput from './ScmProjectInput.vue';
import type { ScmProject } from '../types';

const projects: ScmProject[] = Array.from({ length: 8 }, (_, index) => ({
  id: String(101 + index),
  name: index === 7 ? '最后一个非常长的项目名称 Last Project' : `项目 ${index + 1}`,
  status: null,
}));
const cleanup: (() => void)[] = [];
afterEach(() => cleanup.splice(0).forEach((close) => close()));

function mount(
  overrides: { modelValue?: string; projects?: ScmProject[]; disabled?: boolean } = {},
) {
  const root = document.createElement('div');
  document.body.append(root);
  const props = reactive({
    id: 'scm-test',
    label: 'SCM 项目',
    modelValue: '',
    projects,
    disabled: false,
    ...overrides,
  });
  const changed = vi.fn((value: string) => {
    props.modelValue = value;
  });
  const app = createApp({
    render: () => h(ScmProjectInput, { ...props, 'onUpdate:modelValue': changed }),
  });
  app.mount(root);
  cleanup.push(() => {
    app.unmount();
    root.remove();
  });
  const input = root.querySelector('input')!;
  return {
    root,
    props,
    changed,
    input,
    options: () => [...root.querySelectorAll<HTMLElement>('[role="option"]')],
    list: () => root.querySelector<HTMLElement>('[role="listbox"]'),
    async focus() {
      input.focus();
      await nextTick();
    },
    async type(value: string) {
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await nextTick();
    },
    async key(key: string) {
      input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
      await nextTick();
    },
  };
}

it('以 label 为根，已选 ID 展开仍保留八个项目并提供 combobox 关联', async () => {
  const panel = mount({ modelValue: '101' });
  expect(panel.root.firstElementChild?.tagName).toBe('LABEL');
  expect(panel.input.hasAttribute('data-scm-project-input')).toBe(true);
  expect(panel.input.getAttribute('autocomplete')).toBe('off');
  expect(panel.input.getAttribute('aria-expanded')).toBe('false');
  await panel.focus();
  expect(panel.options().map((option) => option.dataset.projectId)).toEqual(
    projects.map((p) => p.id),
  );
  expect(panel.input.getAttribute('aria-controls')).toBe(panel.list()?.id);
  expect(panel.options()[7].textContent).toContain(projects[7].name);
  expect(panel.options()[7].title).toContain(projects[7].name);
});

it('选择末项写入 ID 并收起，点击输入框重新显示全部项目', async () => {
  const panel = mount();
  await panel.focus();
  panel.options()[7].click();
  await nextTick();
  expect(panel.changed).toHaveBeenLastCalledWith('108');
  expect(panel.input.value).toBe('108');
  expect(panel.list()).toBeNull();
  panel.input.click();
  await nextTick();
  expect(panel.options()).toHaveLength(8);
});

it('输入按 ID 或名称搜索，逐字输入保持焦点，未匹配 URL 原样保留', async () => {
  const panel = mount();
  await panel.focus();
  await panel.type('107');
  expect(panel.options().map((option) => option.dataset.projectId)).toEqual(['107']);
  await panel.type('last project');
  expect(panel.options().map((option) => option.dataset.projectId)).toEqual(['108']);
  expect(document.activeElement).toBe(panel.input);
  const url = 'https://scm.example.invalid/project/999?branch=main';
  await panel.type(url);
  expect(panel.options()).toHaveLength(0);
  expect(panel.root.textContent).toContain('仍可手动输入');
  expect(panel.props.modelValue).toBe(url);
  await panel.key('Enter');
  expect(panel.props.modelValue).toBe(url);
});

it('无候选仍支持手工项目 ID 和 URL', async () => {
  const panel = mount({ projects: [] });
  await panel.focus();
  await panel.type('900');
  expect(panel.props.modelValue).toBe('900');
  await panel.type('https://scm.example.invalid/project/900');
  expect(panel.props.modelValue).toBe('https://scm.example.invalid/project/900');
});

it('方向键保持焦点并滚动候选，Enter 选择，Escape 与 Tab 关闭', async () => {
  const panel = mount();
  await panel.focus();
  const list = panel.list()!;
  Object.defineProperty(list, 'clientHeight', { value: 216 });
  panel.options().forEach((option, index) => {
    Object.defineProperty(option, 'offsetTop', { value: index * 36 });
    Object.defineProperty(option, 'offsetHeight', { value: 36 });
  });
  await panel.key('ArrowUp');
  expect(panel.input.getAttribute('aria-activedescendant')).toBe(panel.options()[7].id);
  expect(list.scrollTop).toBe(72);
  expect(document.activeElement).toBe(panel.input);
  await panel.key('ArrowDown');
  expect(list.scrollTop).toBe(0);
  await panel.key('Enter');
  expect(panel.props.modelValue).toBe('101');
  expect(panel.list()).toBeNull();
  await panel.key('ArrowDown');
  expect(panel.options()).toHaveLength(8);
  await panel.key('Escape');
  expect(panel.list()).toBeNull();
  expect(panel.input.hasAttribute('aria-activedescendant')).toBe(false);
  panel.input.click();
  await nextTick();
  await panel.key('Tab');
  expect(panel.list()).toBeNull();
});

it('列表滚轮与滚动条指针不收起，外部指针或焦点关闭', async () => {
  const panel = mount();
  await panel.focus();
  panel.list()!.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
  panel.list()!.dispatchEvent(new WheelEvent('wheel', { deltaY: 100, bubbles: true }));
  await nextTick();
  expect(panel.list()).not.toBeNull();
  document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
  await nextTick();
  expect(panel.list()).toBeNull();
  panel.input.click();
  await nextTick();
  document.body.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
  await nextTick();
  expect(panel.list()).toBeNull();
});

it('禁用时不展开、不写值，展开后禁用立即关闭', async () => {
  const panel = mount({ disabled: true });
  panel.input.dispatchEvent(new FocusEvent('focus'));
  panel.input.dispatchEvent(new MouseEvent('click'));
  await panel.key('ArrowDown');
  await panel.type('101');
  expect(panel.list()).toBeNull();
  expect(panel.changed).not.toHaveBeenCalled();
  panel.props.disabled = false;
  await nextTick();
  await panel.focus();
  const option = panel.options()[0];
  panel.props.disabled = true;
  await nextTick();
  option.click();
  expect(panel.list()).toBeNull();
  expect(panel.changed).not.toHaveBeenCalled();
});
