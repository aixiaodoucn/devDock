<script lang="ts">
import type { BackendRepository } from '../types';
import { isAbsolutePath, pathKey } from '../platform';
// 显式选取八个元数据字段，扫描结果中的凭据等额外字段不进入配置。
export function repositoryMetadata(source: BackendRepository): BackendRepository {
  return {
    id: source.id,
    name: source.name,
    path: source.path,
    remote: source.remote ?? 'origin',
    scmGitId: source.scmGitId ?? '',
    scmModuleName: source.scmModuleName ?? '',
    developmentBranch: source.developmentBranch ?? '',
    testBranch: source.testBranch ?? '',
  };
}
export function backendRepositoriesError(repositories: BackendRepository[]): string {
  const ids = new Set<string>();
  const paths = new Set<string>();
  for (const [index, repository] of repositories.entries()) {
    const prefix = `子项目 ${index + 1}：`;
    if (Object.values(repositoryMetadata(repository)).some((value) => value.includes('\0')))
      return `${prefix}字段不能含 NUL。`;
    if (!repository.id.trim() || !repository.name.trim()) return `${prefix}ID 和名称不能为空。`;
    if (!isAbsolutePath(repository.path)) return `${prefix}请填写完整的绝对路径。`;
    if (ids.has(repository.id)) return `${prefix}ID 重复。`;
    if (paths.has(pathKey(repository.path))) return `${prefix}目录重复。`;
    ids.add(repository.id);
    paths.add(pathKey(repository.path));
  }
  return '';
}
</script>
<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue';
import { Plus, RefreshCw, Trash2 } from 'lucide-vue-next';
import type { BackendCandidate } from '../types';
import { api, desktop } from '../api';
import PathInput from './PathInput.vue';
import GitBranch from './GitBranch.vue';
const props = defineProps<{ parentPath: string; modelValue: BackendRepository[] }>();
const emit = defineEmits<{ 'update:modelValue': [value: BackendRepository[]] }>();
const candidates = ref<(BackendCandidate & { selected: boolean })[]>([]);
const scanning = ref(false);
const notice = ref('');
const error = ref('');
let requestId = 0;
async function scan() {
  const request = ++requestId;
  candidates.value = [];
  error.value = '';
  notice.value = '';
  scanning.value = false;
  const path = props.parentPath.trim();
  if (!desktop || !isAbsolutePath(path)) return;
  scanning.value = true;
  try {
    const result = await api.scanBackendChildren(path);
    if (request !== requestId) return;
    // 保留扫描的只读结果供用户确认，绝不自动追加到项目组。
    candidates.value = result.map((item) => ({
      id: item.id,
      name: item.name,
      path: item.path,
      isGit: item.isGit,
      currentBranch: item.currentBranch,
      error: item.error,
      selected: item.isGit,
    }));
    notice.value = result.length
      ? '扫描完成，请确认名称和目录后添加。普通目录仅记录目录，不参与 Git 操作。'
      : '未发现一级子目录。';
  } catch (cause) {
    if (request === requestId) error.value = String(cause);
  } finally {
    if (request === requestId) scanning.value = false;
  }
}
watch(() => props.parentPath, scan, { immediate: true });
onUnmounted(() => {
  requestId++;
});
function update(index: number, field: keyof BackendRepository, value: string) {
  emit(
    'update:modelValue',
    props.modelValue.map((item, i) =>
      repositoryMetadata(i === index ? { ...item, [field]: value } : item),
    ),
  );
}
function addSelected() {
  const next = props.modelValue.map(repositoryMetadata);
  const paths = new Set(next.map((item) => pathKey(item.path)));
  let skipped = 0;
  for (const item of candidates.value.filter((item) => item.selected)) {
    if (paths.has(pathKey(item.path))) {
      skipped++;
      continue;
    }
    paths.add(pathKey(item.path));
    next.push(
      repositoryMetadata({
        id: crypto.randomUUID(),
        name: item.name.trim(),
        path: item.path.trim(),
        remote: 'origin',
        scmGitId: '',
        scmModuleName: '',
        developmentBranch: '',
        testBranch: '',
      }),
    );
  }
  error.value = backendRepositoriesError(next);
  if (error.value) return;
  emit('update:modelValue', next);
  notice.value = skipped ? `已保留已有子项目，跳过 ${skipped} 个重复目录。` : '已添加选中子项目。';
}
function addManual() {
  emit('update:modelValue', [
    ...props.modelValue,
    {
      id: crypto.randomUUID(),
      name: '',
      path: '',
      remote: 'origin',
      scmGitId: '',
      scmModuleName: '',
      developmentBranch: '',
      testBranch: '',
    },
  ]);
}
</script>
<template>
  <section class="form-stack backend-repositories" aria-label="后端子项目">
    <div class="section-heading">
      <div>
        <h3>子项目</h3>
        <p>扫描父目录下一级目录，确认后加入项目组。</p>
      </div>
      <button
        type="button"
        class="button small"
        :disabled="scanning || !desktop || !isAbsolutePath(parentPath.trim())"
        @click="scan"
      >
        <RefreshCw :size="14" :class="{ spinning: scanning }" />{{
          scanning ? '扫描中…' : '重新扫描'
        }}
      </button>
    </div>
    <p class="field-help">
      更换父目录不会删除已保存子项目；移除记录不会删除本地目录。仅保存元数据，不保存密码或 Cookie。
    </p>
    <p v-if="notice" class="field-help" role="status">{{ notice }}</p>
    <div v-if="error" class="inline-message error" role="alert">{{ error }}</div>
    <div
      v-for="(candidate, index) in candidates"
      :key="index"
      class="repository-card candidate-card"
    >
      <label class="checkbox-label"
        ><input v-model="candidate.selected" type="checkbox" />{{
          candidate.isGit ? 'Git 仓库' : '普通目录（不参与 Git 操作）'
        }}</label
      >
      <label>名称<input v-model="candidate.name" :aria-label="`候选 ${index + 1} 名称`" /></label>
      <label
        >目录<PathInput
          v-model="candidate.path"
          :directory="true"
          :label="`候选 ${index + 1} 目录`"
          @error="error = $event"
      /></label>
      <p class="field-help">
        {{
          candidate.error ||
          candidate.currentBranch ||
          (candidate.isGit ? '未创建分支' : '非 Git 仓库')
        }}
      </p>
    </div>
    <button v-if="candidates.length" type="button" class="button" @click="addSelected">
      <Plus :size="14" />添加选中子项目
    </button>
    <div
      v-for="(repository, index) in modelValue"
      :key="repository.id"
      class="repository-card saved-repository"
    >
      <div class="section-heading">
        <strong>子项目 {{ index + 1 }}</strong
        ><button
          type="button"
          class="icon-button danger-text"
          :aria-label="`移除子项目 ${index + 1}`"
          @click="
            emit(
              'update:modelValue',
              modelValue.filter((_, i) => i !== index),
            )
          "
        >
          <Trash2 :size="15" />
        </button>
      </div>
      <label
        >名称<input
          :value="repository.name"
          :aria-label="`子项目 ${index + 1} 名称`"
          @input="update(index, 'name', ($event.target as HTMLInputElement).value)"
      /></label>
      <label
        >目录<PathInput
          :model-value="repository.path"
          :directory="true"
          :label="`子项目 ${index + 1} 目录`"
          @update:model-value="update(index, 'path', $event)"
          @error="error = $event"
      /></label>
      <GitBranch :path="repository.path" />
      <div class="form-grid">
        <label
          >Remote<input
            :value="repository.remote"
            :aria-label="`子项目 ${index + 1} Remote`"
            @input="update(index, 'remote', ($event.target as HTMLInputElement).value)"
        /></label>
        <label
          >SCM Git ID<input
            :value="repository.scmGitId"
            :aria-label="`子项目 ${index + 1} SCM Git ID`"
            @input="update(index, 'scmGitId', ($event.target as HTMLInputElement).value)"
        /></label>
      </div>
      <label
        >SCM 模块名称<input
          :value="repository.scmModuleName"
          :aria-label="`子项目 ${index + 1} SCM 模块名称`"
          @input="update(index, 'scmModuleName', ($event.target as HTMLInputElement).value)"
      /></label>
    </div>
    <button type="button" class="button small" @click="addManual">
      <Plus :size="14" />手动添加子项目
    </button>
  </section>
</template>
<style scoped>
.repository-card {
  display: grid;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--accent-border);
  border-radius: 8px;
  min-width: 0;
}
.candidate-card {
  background: var(--accent-soft);
}
.repository-card .field-help {
  margin: 0;
  overflow-wrap: anywhere;
}
</style>
