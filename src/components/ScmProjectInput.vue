<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { ScmProject } from '../types';

const props = defineProps<{
  modelValue: string;
  projects: ScmProject[];
  label: string;
  id: string;
  disabled?: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();
const root = ref<HTMLLabelElement>();
const list = ref<HTMLElement>();
const open = ref(false);
const query = ref('');
const activeIndex = ref(-1);
const listId = computed(() => `${props.id}-listbox`);
const filteredProjects = computed(() => {
  const search = query.value.trim().toLocaleLowerCase();
  return props.projects.filter(
    (project) =>
      project.id.toLocaleLowerCase().includes(search) ||
      project.name.toLocaleLowerCase().includes(search),
  );
});
const activeId = computed(() =>
  open.value && filteredProjects.value[activeIndex.value]
    ? `${listId.value}-option-${activeIndex.value}`
    : undefined,
);

function close() {
  open.value = false;
  activeIndex.value = -1;
}
function showAll() {
  if (props.disabled) return;
  // 已选 ID 不参与重新展开时的过滤，方便直接切换项目。
  query.value = '';
  activeIndex.value = -1;
  open.value = true;
}
function onInput(event: Event) {
  if (props.disabled) return;
  const value = (event.target as HTMLInputElement).value;
  emit('update:modelValue', value);
  query.value = value;
  activeIndex.value = -1;
  open.value = true;
}
function select(project: ScmProject) {
  if (props.disabled) return;
  emit('update:modelValue', project.id);
  close();
}
async function onKeydown(event: KeyboardEvent) {
  if (props.disabled || event.isComposing) return;
  if (event.key === 'Escape') {
    if (open.value) event.preventDefault();
    close();
  } else if (event.key === 'Tab') {
    close();
  } else if (event.key === 'Enter') {
    const project = filteredProjects.value[activeIndex.value];
    if (open.value && project) {
      event.preventDefault();
      select(project);
    }
  } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    if (!open.value) showAll();
    const count = filteredProjects.value.length;
    if (!count) return;
    const step = event.key === 'ArrowDown' ? 1 : -1;
    activeIndex.value =
      activeIndex.value < 0
        ? step > 0
          ? 0
          : count - 1
        : (activeIndex.value + step + count) % count;
    await nextTick();
    // 只滚动候选容器，避免键盘导航连带滚动整个页面。
    const option = list.value?.children[activeIndex.value] as HTMLElement | undefined;
    if (!option || !list.value) return;
    const top = option.offsetTop;
    const bottom = top + option.offsetHeight;
    if (top < list.value.scrollTop) list.value.scrollTop = top;
    else if (bottom > list.value.scrollTop + list.value.clientHeight)
      list.value.scrollTop = bottom - list.value.clientHeight;
  }
}
function onOutside(event: Event) {
  if (event.target instanceof Node && !root.value?.contains(event.target)) close();
}
watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) close();
  },
);
watch(
  filteredProjects,
  () => {
    activeIndex.value = -1;
  },
  { flush: 'sync' },
);
onMounted(() => {
  document.addEventListener('pointerdown', onOutside);
  document.addEventListener('focusin', onOutside);
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onOutside);
  document.removeEventListener('focusin', onOutside);
});
</script>

<template>
  <label ref="root" class="scm-project-input" :for="id">
    <span>{{ label }}</span>
    <input
      :id="id"
      data-scm-project-input
      :value="modelValue"
      :disabled="disabled"
      placeholder="项目 ID 或完整 URL"
      role="combobox"
      autocomplete="off"
      aria-autocomplete="list"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="listId"
      :aria-activedescendant="activeId"
      @focus="showAll"
      @click="showAll"
      @input="onInput"
      @keydown="onKeydown"
    />
    <div
      v-if="open"
      :id="listId"
      ref="list"
      class="scm-project-options"
      role="listbox"
      :aria-label="label"
      @click.prevent.stop
    >
      <div
        v-for="(project, index) in filteredProjects"
        :id="`${listId}-option-${index}`"
        :key="project.id"
        role="option"
        class="scm-project-option"
        :class="{ active: activeIndex === index }"
        :data-project-id="project.id"
        :aria-selected="modelValue === project.id"
        :title="`${project.id} · ${project.name}`"
        @mousedown.prevent
        @click.prevent.stop="select(project)"
      >
        <span class="scm-project-id">{{ project.id }}</span>
        <span class="scm-project-name">{{ project.name }}</span>
      </div>
      <div v-if="!filteredProjects.length" class="scm-project-empty" role="status">
        无匹配项目，仍可手动输入项目 ID 或完整 URL
      </div>
    </div>
  </label>
</template>

<style scoped>
.scm-project-input {
  position: relative;
  display: grid;
  gap: 5px;
  min-width: 0;
  color: var(--text-secondary);
  font-size: 12px;
}
.scm-project-input input {
  width: 100%;
  min-width: 0;
}
.scm-project-options {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 20;
  box-sizing: content-box;
  max-height: 216px;
  padding: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--paper);
  box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
}
.scm-project-option {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  height: 36px;
  padding: 0 10px;
  white-space: nowrap;
  cursor: pointer;
}
.scm-project-option:hover,
.scm-project-option.active {
  background: var(--accent-soft);
}
.scm-project-id {
  flex-shrink: 0;
}
.scm-project-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.scm-project-empty {
  padding: 10px;
}
</style>
