<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { Terminal, Settings2, FolderOpen } from 'lucide-vue-next';
import type { AiTool } from '../types';
import { desktop } from '../api';

const props = defineProps<{ tools: AiTool[]; busy: boolean }>();
const emit = defineEmits<{ launch: [tool: AiTool | undefined]; configure: [] }>();
const available = computed(() => props.tools.filter((tool) => tool.enabled));
const preferenceKey = 'dev-dock-preferred-ai-tool';
function preferredTool() {
  try {
    return localStorage.getItem(preferenceKey) ?? '';
  } catch {
    return '';
  }
}
const selectedId = ref(preferredTool());
function rememberTool() {
  try {
    localStorage.setItem(preferenceKey, selectedId.value);
  } catch {
    // 本地存储不可用时仍可选择并启动工具。
  }
}
watch(
  available,
  (tools) => {
    if (!tools.some((tool) => tool.id === selectedId.value)) selectedId.value = '';
  },
  { immediate: true },
);
const selected = computed(() => available.value.find((tool) => tool.id === selectedId.value));
function launch() {
  if (!props.busy && desktop) emit('launch', selected.value);
}
function openDirectory() {
  // 目录入口始终不传工具，保留用户当前选择的常用 AI。
  if (!props.busy && desktop) emit('launch', undefined);
}
</script>

<template>
  <section class="ai-launcher" aria-label="项目终端入口">
    <div class="ai-launcher-copy">
      <Terminal :size="16" />
      <div><strong>在终端中打开</strong><small>进入项目目录，可选启动 AI 工具</small></div>
    </div>
    <div class="ai-launcher-controls">
      <select
        v-model="selectedId"
        aria-label="选择 AI 工具（可选）"
        :disabled="busy"
        @change="rememberTool"
      >
        <option value="">仅打开项目目录</option>
        <option v-for="tool in available" :key="tool.id" :value="tool.id">{{ tool.name }}</option>
      </select>
      <button
        class="button primary small"
        :title="selected?.command || '在终端中打开项目目录'"
        :disabled="busy || !desktop"
        @click="launch"
      >
        <Terminal :size="14" />打开
      </button>
      <button
        class="button small directory-only"
        :disabled="busy || !desktop"
        @click="openDirectory"
      >
        <FolderOpen :size="14" />仅打开目录
      </button>
      <button
        class="icon-button"
        aria-label="配置 AI 工具"
        title="配置 AI 工具"
        @click="emit('configure')"
      >
        <Settings2 :size="15" />
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
