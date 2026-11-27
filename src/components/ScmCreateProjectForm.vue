<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type {
  ScmCreateProjectInput,
  ScmCreateProjectOptions,
  ScmEditProjectOptions,
} from '../types';
const props = defineProps<{
  options: ScmCreateProjectOptions;
  disabled: boolean;
  edit?: ScmEditProjectOptions;
  projectId?: string;
}>();
const emit = defineEmits<{
  submit: [input: ScmCreateProjectInput];
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
const moduleSearch = ref('');
const existingModules = computed(() => new Set(props.edit?.existingModules));
watch(
  [() => props.edit, () => props.projectId],
  ([edit]) => {
    // 现有绑定单独展示，modules 只记录本次追加的模块。
    form.value = edit ? { ...edit.input, modules: [] } : createDefaults();
    authors.value = [...new Set(edit?.authors)];
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
const canSubmit = computed(
  () =>
    !props.disabled &&
    !!form.value.name &&
    !!form.value.projectType &&
    (props.edit ? authors.value.length > 0 : !!form.value.user),
);
function submitForm() {
  if (!canSubmit.value) return;
  const input = { ...form.value, modules: [...form.value.modules] };
  if (props.edit) emit('update', { ...input, user: authors.value[0] }, [...authors.value]);
  else emit('submit', input);
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
  <form :aria-label="title" @submit.prevent="submitForm">
    <fieldset :disabled="disabled">
      <legend>
        {{ title }}<template v-if="edit && projectId"> · #{{ projectId }}</template>
      </legend>
      <div class="fields">
        <label>项目名称<input v-model.trim="form.name" name="name" required /></label>
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
        <label v-if="edit"
          >负责人（可多选）
          <select v-model="authors" name="authors" multiple required>
            <option v-for="item in authorOptions" :key="item.value" :value="item.value">
              {{ item.label }}
            </option>
          </select>
        </label>
        <label v-else
          >负责人<select v-model="form.user" name="user" required>
            <option value="" disabled>请选择负责人</option>
            <option v-for="item in options.users" :key="item.value" :value="item.value">
              {{ item.label }}
            </option>
          </select></label
        >
      </div>
      <label>项目详情<textarea v-model="form.details" name="details" rows="2" /></label>
      <fieldset class="modules">
        <legend>{{ edit ? '新增模块' : '项目模块' }}</legend>
        <div v-if="edit" class="existing-modules" role="group" aria-label="已有模块">
          <strong>已有模块（{{ existingModules.size }}）</strong>
          <p>已有模块只读，保存时仅追加下方所选模块。</p>
          <ul v-if="existingModules.size">
            <li v-for="id in existingModules" :key="id">{{ moduleLabels.get(id) || id }}</li>
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
      <div class="actions">
        <button type="submit" class="button small primary" :disabled="!canSubmit">
          {{ edit ? '保存修改' : '确认创建' }}</button
        ><button type="button" class="button small" @click="emit('cancel')">取消</button>
      </div>
    </fieldset>
  </form>
</template>
<style scoped>
form {
  background: var(--paper);
  border: 1px solid var(--line);
  border-radius: 7px;
  padding: 14px;
  margin-bottom: 12px;
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
  margin-right: 12px;
}
</style>
