<script setup lang="ts">
import { computed } from 'vue';
import { Terminal, Settings2, FolderOpen } from 'lucide-vue-next';
import type { AiTool } from '../types';
import { desktop } from '../api';
import { resolveAiTool } from '../ai';

const props = defineProps<{
  tools: AiTool[];
  busy: boolean;
  projectToolId: string;
  defaultToolId: string;
}>();
const emit = defineEmits<{
  launch: [tool: AiTool | undefined];
  configure: [];
  select: [toolId: string];
}>();
const available = computed(() => props.tools.filter((tool) => tool.enabled));
const defaultTool = computed(() => resolveAiTool(props.tools, '', props.defaultToolId));
const selected = computed(() =>
  resolveAiTool(props.tools, props.projectToolId, props.defaultToolId),
);
const unavailable = computed(
  () => !!props.projectToolId && !available.value.some((tool) => tool.id === props.projectToolId),
);
const selectionHint = computed(() => {
  if (unavailable.value) return '当前项目的终端命令已移除或禁用，请重新选择';
  return available.value.length
    ? '请设置全局默认命令，或为当前项目单独选择'
    : '请先在偏好设置配置并启用终端命令';
});
function selectTool(event: Event) {
  emit('select', (event.target as HTMLSelectElement).value);
}
function launch() {
  if (props.busy || !desktop) return;
  if (!selected.value) {
    emit('configure');
    return;
  }
  emit('launch', selected.value);
}
function openDirectory() {
  // 目录入口始终不传工具，保留用户当前选择的常用命令。
  if (!props.busy && desktop) emit('launch', undefined);
}
</script>

<template>
  <section class="ai-launcher" aria-label="项目终端入口">
    <div class="ai-launcher-copy">
      <Terminal :size="16" />
      <div>
        <strong>打开并执行命令</strong>
        <small>{{
          selected
            ? `${projectToolId ? '当前项目专用' : '跟随全局默认'} · ${selected.name}`
            : selectionHint
        }}</small>
      </div>
    </div>
    <div class="ai-launcher-controls">
      <button
        class="icon-button"
        aria-label="配置终端命令"
        title="配置终端命令"
        @click="emit('configure')"
      >
        <Settings2 :size="15" />
      </button>
      <select
        :value="projectToolId"
        aria-label="项目专用命令"
        :disabled="busy"
        @change="selectTool"
      >
        <option value="">
          跟随全局默认{{ defaultTool ? `（${defaultTool.name}）` : '（未配置）' }}
        </option>
        <option v-if="unavailable" :value="projectToolId" disabled>
          项目命令不可用，请重新选择
        </option>
        <option v-for="tool in available" :key="tool.id" :value="tool.id">{{ tool.name }}</option>
      </select>
      <button
        class="button primary small"
        :title="selected?.command || selectionHint"
        :disabled="busy || !desktop"
        @click="launch"
      >
        <Terminal :size="14" />打开并执行
      </button>
      <button
        class="button small directory-only"
        :disabled="busy || !desktop"
        @click="openDirectory"
      >
        <FolderOpen :size="14" />仅打开目录
      </button>
    </div>
  </section>
</template>

<style scoped>
.ai-launcher {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 12px;
  padding: 12px 14px;
  margin-bottom: 18px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--paper);
}
.ai-launcher-copy,
.ai-launcher-controls {
  display: flex;
  align-items: center;
  gap: 9px;
  min-width: 0;
}
.ai-launcher-copy > svg {
  color: var(--accent-text);
  flex-shrink: 0;
}
.ai-launcher-copy strong {
  display: block;
  font-size: 12px;
  font-weight: 600;
}
.ai-launcher-copy small {
  display: block;
  color: var(--muted);
  font-size: 10px;
  margin-top: 3px;
}
.ai-launcher-controls select,
.ai-launcher-controls button {
  height: 32px;
  min-height: 32px;
  padding-top: 0;
  padding-bottom: 0;
}
.ai-launcher-controls select {
  width: 145px;
  font-size: 12px;
}
.ai-launcher-controls button {
  flex-shrink: 0;
}
</style>
