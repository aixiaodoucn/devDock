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
    // 已保存目录只显示一次，勾选结果在父弹框保存时统一收集。
    const savedPaths = new Set(props.modelValue.map((item) => pathKey(item.path)));
    candidates.value = result
      .filter((item) => !savedPaths.has(pathKey(item.path)))
      .map((item) => ({
        id: item.id,
        name: item.name,
        path: item.path,
        isGit: item.isGit,
        currentBranch: item.currentBranch,
        error: item.error,
        selected: item.isGit,
      }));
    notice.value = result.length
      ? '勾选需要添加的目录，点击“保存项目”生效。普通目录不参与 Git 操作。'
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
function collectRepositories(): BackendRepository[] {
  const next = props.modelValue.map(repositoryMetadata);
  const paths = new Set(next.map((item) => pathKey(item.path)));
  for (const item of candidates.value.filter((item) => item.selected)) {
    if (paths.has(pathKey(item.path.trim()))) continue;
    paths.add(pathKey(item.path.trim()));
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
  return next;
}
defineExpose({ collectRepositories });
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
        <p>勾选一级子目录，保存时加入项目组。</p>
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
    <p class="field-help">更换父目录保留已保存子项目；移除记录不会删除本地目录。</p>
    <p v-if="notice" class="field-help" role="status">{{ notice }}</p>
    <div v-if="error" class="inline-message error" role="alert">{{ error }}</div>
    <div
      v-for="(candidate, index) in candidates"
      :key="index"
      class="repository-card candidate-card"
    >
      <label class="checkbox-label"
        ><input
          v-model="candidate.selected"
          type="checkbox"
          :aria-label="`选择子项目 ${candidate.name}`"
        />{{ candidate.isGit ? 'Git 仓库' : '普通目录' }}</label
      >
      <div class="repository-fields">
        <label>名称<input v-model="candidate.name" :aria-label="`候选 ${index + 1} 名称`" /></label>
        <label
          >目录<PathInput
            v-model="candidate.path"
            :directory="true"
            :label="`候选 ${index + 1} 目录`"
            @error="error = $event"
        /></label>
      </div>
      <p class="field-help">
        {{
          candidate.error ||
          candidate.currentBranch ||
          (candidate.isGit ? '未创建分支' : '非 Git 仓库')
        }}
      </p>
    </div>
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
      <div class="repository-fields">
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
      </div>
      <GitBranch :path="repository.path" />
    </div>
    <button type="button" class="button small" @click="addManual">
      <Plus :size="14" />手动添加子项目
    </button>
  </section>
</template>
<style scoped>
.backend-repositories {
  gap: 10px;
}
.repository-card {
  display: grid;
  grid-template-columns: minmax(90px, auto) minmax(0, 1fr);
  align-items: center;
  column-gap: 12px;
  row-gap: 5px;
  padding: 10px 12px;
  border: 1px solid var(--accent-border);
  border-radius: 8px;
  min-width: 0;
}
.repository-fields {
  display: grid;
  grid-template-columns: minmax(110px, 1fr) minmax(0, 2.5fr);
  gap: 10px;
  min-width: 0;
}
.repository-fields label {
  min-width: 0;
}
.repository-card > .field-help,
.repository-card > :deep(.git-branch) {
  grid-column: 2;
}
.candidate-card {
  background: var(--accent-soft);
}
@media (max-width: 600px) {
  .repository-card {
    grid-template-columns: minmax(0, 1fr);
  }
  .repository-card > .field-help,
  .repository-card > :deep(.git-branch) {
    grid-column: 1;
  }
}
.repository-card .field-help {
  margin: 0;
  overflow-wrap: anywhere;
}
</style>
