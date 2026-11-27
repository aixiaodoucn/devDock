<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type {
  ScmCreateProjectInput,
  ScmCreateProjectOptions,
  ScmEditProjectOptions,
} from '../types';
const props = defineProps<{
  options: ScmCreateProjectOptions;
  disabled: boolean;
  error?: string;
  edit?: ScmEditProjectOptions;
  projectId?: string;
}>();
const emit = defineEmits<{
  submit: [input: ScmCreateProjectInput, authors: string[]];
  'remove-module': [moduleId: string];
  update: [input: ScmCreateProjectInput, authors: string[]];
  cancel: [];
}>();
const title = computed(() => (props.edit ? '修改 SCM 项目' : '创建 SCM 项目'));
const choices = computed(() => props.edit?.options ?? props.options);
const createDefaults = (): ScmCreateProjectInput => ({
  name: '',
  branchSuffix: '',
  details: '',
  modules: [],
  projectType: props.options.types[0]?.value ?? '',
  user: props.options.users.some((item) => item.value === props.options.defaultUser)
    ? props.options.defaultUser
    : '',
});
const form = ref(createDefaults());
const authors = ref<string[]>([]);
const dialog = ref<HTMLDialogElement>();
const confirmation = ref<HTMLDialogElement>();
const pendingModule = ref('');
const authorSearch = ref('');
// 原生模态层隔离背景交互，并由浏览器处理焦点循环和关闭时的焦点还原。
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  confirmation.value?.close();
  dialog.value?.close();
});
function requestRemoval(id: string) {
  if (props.disabled || !existingModules.value.has(id)) return;
  pendingModule.value = id;
  confirmation.value?.showModal();
}
function closeConfirmation() {
  confirmation.value?.close();
  pendingModule.value = '';
}
function confirmRemoval() {
  const id = pendingModule.value;
  closeConfirmation();
  if (!props.disabled && existingModules.value.has(id)) emit('remove-module', id);
}
const moduleSearch = ref('');
const existingModules = computed(() => new Set(props.edit?.existingModules));
watch(
  [() => props.edit, () => props.projectId],
  ([edit]) => {
    // 现有绑定单独展示，modules 只记录本次追加的模块。
    form.value = edit ? { ...edit.input, modules: [] } : createDefaults();
    authors.value = edit ? [...new Set(edit.authors)] : form.value.user ? [form.value.user] : [];
    authorSearch.value = '';
    closeConfirmation();
    moduleSearch.value = '';
  },
  { immediate: true },
);
const authorOptions = computed(() => {
  const labels = new Map(choices.value.users.map((item) => [item.value, item.label]));
  // 原负责人可能已不在候选名单中，仍需保留其身份和顺序。
  return [...new Set([...(props.edit?.authors ?? []), ...labels.keys()])].map((value) => ({
    value,
    label: labels.get(value) ?? value,
  }));
});
const filteredAuthors = computed(() => {
  const query = authorSearch.value.trim().toLowerCase();
  return authorOptions.value.filter(
    ({ value, label }) =>
      value.toLowerCase().includes(query) || label.toLowerCase().includes(query),
  );
});
const authorLabels = computed(
  () => new Map(authorOptions.value.map(({ value, label }) => [value, label])),
);
const canSubmit = computed(
  () =>
    !props.disabled && !!form.value.name && !!form.value.projectType && authors.value.length > 0,
);
function submitForm() {
  if (!canSubmit.value) return;
  const input = { ...form.value, user: authors.value[0], modules: [...form.value.modules] };
  if (props.edit) emit('update', input, [...authors.value]);
  else emit('submit', input, [...authors.value]);
}
function addModule(value: string) {
  if (!existingModules.value.has(value) && !form.value.modules.includes(value))
    form.value.modules.push(value);
}
const filteredModules = computed(() => {
  const query = moduleSearch.value.trim().toLowerCase();
  return choices.value.modules.filter(
    (item) =>
      !existingModules.value.has(item.value) &&
      (item.label.toLowerCase().includes(query) || item.value.toLowerCase().includes(query)),
  );
});
// 已选列表独立于搜索结果，按用户添加顺序展示。
const moduleLabels = computed(
  () => new Map(choices.value.modules.map((item) => [item.value, item.label])),
);
function removeModule(value: string) {
  form.value.modules = form.value.modules.filter((id) => id !== value);
}
</script>
<template>
  <dialog
    ref="dialog"
    class="project-dialog"
    :aria-label="title"
    @cancel.prevent="!disabled && emit('cancel')"
  >
    <form :aria-label="title" @submit.prevent="submitForm">
      <header>
        <span class="eyebrow">SCM / 项目管理</span>
        <h2>
          {{ title }} <small v-if="edit && projectId">#{{ projectId }}</small>
        </h2>
      </header>
      <div class="scroll-body">
        <fieldset :disabled="disabled">
          <div class="fields">
            <label>项目名称<input v-model.trim="form.name" name="name" autofocus required /></label>
            <label
              >分支后缀（可选）<input
                v-model.trim="form.branchSuffix"
                name="branchSuffix"
                pattern="[A-Za-z0-9_.]+"
                title="可使用英文字母、数字、下划线或点"
            /></label>
            <label
              >项目类型<select v-model="form.projectType" name="projectType" required>
                <option v-for="item in choices.types" :key="item.value" :value="item.value">
                  {{ item.label }}
                </option>
              </select></label
            >
          </div>
          <label>项目详情<textarea v-model="form.details" name="details" rows="2" /></label>
          <fieldset class="authors">
            <legend>负责人（可多选）</legend>
            <label
              >搜索负责人<input
                type="search"
                name="authorSearch"
                v-model="authorSearch"
                placeholder="按姓名或账号搜索"
            /></label>
            <div class="author-list" role="group" aria-label="候选负责人">
              <label v-for="item in filteredAuthors" :key="item.value"
                ><input
                  type="checkbox"
                  name="authors"
                  v-model="authors"
                  :value="item.value"
                /><span>{{ item.label }}</span
                ><small v-if="item.label !== item.value">{{ item.value }}</small></label
              >
              <span v-if="!filteredAuthors.length" role="status">没有匹配的负责人</span>
            </div>
            <div class="module-tags" role="group" aria-label="已选负责人">
              <strong>已选 {{ authors.length }} 人</strong>
              <button
                v-for="id in authors"
                :key="id"
                type="button"
                class="button small"
                :aria-label="`移除负责人 ${id}`"
                @click="authors = authors.filter((value) => value !== id)"
              >
                {{ authorLabels.get(id) || id }} <span aria-hidden="true">×</span>
              </button>
            </div>
          </fieldset>
          <fieldset class="modules">
            <legend>{{ edit ? '新增模块' : '项目模块' }}</legend>
            <div v-if="edit" class="existing-modules" role="group" aria-label="已有模块">
              <strong>已有模块（{{ existingModules.size }}）</strong>
              <p>解除关联立即生效；下方新增的模块在保存时关联。</p>
              <ul v-if="existingModules.size">
                <li v-for="id in existingModules" :key="id">
                  <span>{{ moduleLabels.get(id) || id }}</span
                  ><button
                    type="button"
                    class="button small"
                    :aria-label="`解除关联 ${moduleLabels.get(id) || id}`"
                    @click="requestRemoval(id)"
                  >
                    解除关联
                  </button>
                </li>
              </ul>
              <span v-else>暂无已有模块</span>
            </div>
            <div class="module-columns">
              <div>
                <label
                  >搜索模块<input
                    type="search"
                    v-model="moduleSearch"
                    name="moduleSearch"
                    placeholder="按模块名称或编号搜索"
                /></label>
                <div class="module-list" role="group" aria-label="选择模块">
                  <button
                    v-for="item in filteredModules"
                    :key="item.value"
                    type="button"
                    :disabled="form.modules.includes(item.value)"
                    @click="addModule(item.value)"
                  >
                    {{ item.label }}<span v-if="form.modules.includes(item.value)"> · 已选</span>
                  </button>
                  <span v-if="!filteredModules.length" role="status">没有匹配的模块</span>
                </div>
              </div>
              <div role="group" aria-label="已选模块">
                <strong>已选模块（{{ form.modules.length }}）</strong>
                <div class="module-tags">
                  <span v-if="!form.modules.length">尚未选择模块</span>
                  <button
                    v-for="id in form.modules"
                    :key="id"
                    type="button"
                    class="button small"
                    :aria-label="`移除${moduleLabels.get(id)}`"
                    @click="removeModule(id)"
                  >
                    {{ moduleLabels.get(id) }} <span aria-hidden="true">×</span>
                  </button>
                </div>
              </div>
            </div>
          </fieldset>
        </fieldset>
      </div>
      <footer>
        <p v-if="error" role="alert">{{ error }}</p>
        <div class="actions">
          <button type="submit" class="button small primary" :disabled="!canSubmit">
            {{ edit ? '保存修改' : '确认创建' }}</button
          ><button type="button" class="button small" :disabled="disabled" @click="emit('cancel')">
            取消
          </button>
        </div>
      </footer>
    </form>
    <dialog
      ref="confirmation"
      role="alertdialog"
      class="confirmation"
      aria-label="确认解除模块关联"
      aria-describedby="scm-unlink-note"
      @cancel.stop.prevent="closeConfirmation"
    >
      <h3>解除模块关联？</h3>
      <p id="scm-unlink-note">
        将立即解除「{{ moduleLabels.get(pendingModule) || pendingModule }}」与本项目的关联，不会删除
        Git 仓库。无需保存表单。
      </p>
      <div class="actions">
        <button
          type="button"
          class="button small"
          data-action="cancel"
          autofocus
          @click="closeConfirmation"
        >
          取消</button
        ><button
          type="button"
          class="button small"
          data-action="confirm"
          :disabled="disabled"
          @click="confirmRemoval"
        >
          立即解除关联
        </button>
      </div>
    </dialog>
  </dialog>
</template>
<style scoped>
dialog {
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: 0 20px 64px color-mix(in srgb, var(--ink) 24%, transparent);
  background: var(--paper);
  color: var(--ink);
  margin: auto;
}
dialog::backdrop {
  background: color-mix(in srgb, var(--ink) 44%, transparent);
}
.project-dialog {
  width: min(44rem, calc(100vw - 32px));
  padding: 0;
  max-height: none;
  overflow: hidden;
}
form {
  display: flex;
  flex-direction: column;
  max-height: calc(100dvh - 32px);
}
header,
footer {
  flex: none;
  padding: 12px 20px;
}
header {
  border-bottom: 1px solid var(--line);
}
.eyebrow,
h2 small,
.author-list small {
  color: var(--muted);
  font: 11px var(--mono);
}
h2 {
  margin: 3px 0 0;
  font-size: 18px;
}
.scroll-body {
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: 16px 20px;
}
footer {
  border-top: 1px solid var(--line);
  text-align: right;
}
footer p {
  margin: 0 0 8px;
  text-align: left;
  color: var(--danger);
  overflow-wrap: anywhere;
}
textarea[name='details'] {
  height: 3.5rem;
  min-height: 0;
  max-height: 8rem;
}
.authors {
  margin-block: 14px;
}
.author-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(13rem, 100%), 1fr));
  max-height: 9rem;
  overflow: auto;
  padding: 6px;
  margin-top: 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
}
.author-list label,
.existing-modules li {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px;
  overflow-wrap: anywhere;
}
.author-list input {
  width: 1rem;
  height: 1rem;
  flex: none;
}
.author-list small {
  overflow-wrap: anywhere;
}
.authors .module-tags {
  align-items: center;
  font-size: 12px;
}
.confirmation {
  width: min(25rem, calc(100vw - 32px));
  padding: 18px;
}
.confirmation h3 {
  margin: 0;
}
.confirmation p {
  font-size: 13px;
  line-height: 1.6;
  overflow-wrap: anywhere;
}
.confirmation [data-action='confirm'] {
  color: var(--danger);
}
fieldset {
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
}
legend {
  padding: 0;
  margin-bottom: 8px;
  font-weight: 600;
}
.fields {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 16rem), 1fr));
  gap: 8px 12px;
  margin-bottom: 8px;
}
label {
  display: grid;
  gap: 4px;
  font-size: 12px;
}
.modules {
  margin-block: 10px;
  display: grid;
  gap: 6px;
}
.existing-modules {
  border-inline-start: 2px solid var(--line);
  padding-inline-start: 10px;
  font-size: 12px;
}
.existing-modules p {
  margin: 4px 0;
}
.existing-modules ul {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  list-style: none;
  margin: 0;
  padding: 0;
}
.module-columns {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 13rem), 1fr));
  gap: 12px;
  font-size: 12px;
}
.module-list,
.module-tags {
  max-height: 11rem;
  overflow: auto;
  margin-top: 6px;
  padding: 4px;
}
.module-list {
  display: grid;
  border: 1px solid var(--line);
  border-radius: 6px;
}
.module-list button {
  text-align: left;
  padding: 5px 8px;
  border: 0;
  background: transparent;
  color: inherit;
}
.module-list button:hover:not(:disabled) {
  background: var(--line);
}
.module-list button:disabled {
  opacity: 0.55;
}
.module-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.module-tags button {
  max-width: 100%;
  overflow-wrap: anywhere;
}

.actions {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
