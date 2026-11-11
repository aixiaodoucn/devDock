<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { GitBranch as GitBranchIcon } from 'lucide-vue-next';
import { api, desktop } from '../api';

const props = defineProps<{ path: string }>();
const label = ref('');
const error = ref('');
let requestId = 0;

async function refresh() {
  const request = ++requestId;
  error.value = '';
  if (!desktop) {
    label.value = '桌面端查看分支';
    return;
  }
  label.value = '读取分支…';
  try {
    const branch = await api.gitBranch(props.path);
    // 切换项目或卸载后，旧请求不得覆盖当前项目的分支。
    if (request === requestId) label.value = branch;
  } catch (cause) {
    if (request !== requestId) return;
    label.value = '分支读取失败';
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
}
watch(() => props.path, refresh, { immediate: true });
onMounted(() => window.addEventListener('focus', refresh));
onUnmounted(() => {
  requestId++;
  window.removeEventListener('focus', refresh);
});
</script>

<template>
  <span class="git-branch" :title="error || label" role="status" aria-label="当前 Git 分支">
    <GitBranchIcon :size="13" aria-hidden="true" />
    <span>{{ label }}</span>
  </span>
</template>

<style scoped>
.git-branch {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 240px;
  min-width: 0;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--accent-border);
  border-radius: 5px;
  background: var(--accent-soft);
  color: var(--accent-text);
  font-family: var(--mono);
  font-size: 11px;
  font-weight: 600;
  line-height: 1.3;
  vertical-align: middle;
}
.git-branch svg {
  flex-shrink: 0;
}
.git-branch span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
