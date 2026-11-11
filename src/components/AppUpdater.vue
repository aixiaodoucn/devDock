<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { version } from '../../package.json';
import { desktop } from '../api';

type UpdateInfo = { version: string; notes: string | null; downloaded: boolean };
type DownloadProgress = { downloaded: number; total: number | null };
type Phase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'installing';
const props = defineProps<{ busy?: boolean }>();
const CHECK_INTERVAL = 4 * 60 * 60 * 1000;
const phase = ref<Phase>('idle');
const update = ref<UpdateInfo | null>(null);
const progress = ref<DownloadProgress>({ downloaded: 0, total: null });
const checkError = ref('');
const actionError = ref('');
let disposed = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let channel: Channel<DownloadProgress> | undefined;
const transferActive = computed(() => ['downloading', 'ready', 'installing'].includes(phase.value));
const checkDisabled = computed(
  () => !desktop || phase.value === 'checking' || transferActive.value,
);
const versionTitle = computed(() => {
  if (!desktop) return `当前版本 v${version}；桌面应用支持检查更新`;
  if (phase.value === 'checking') return '正在检查更新';
  if (transferActive.value) return '更新处理中，暂不检查新版本';
  if (checkError.value) return `检查失败：${checkError.value}；点击版本号重试`;
  return `当前版本 v${version}；点击检查更新`;
});
const installTitle = computed(() =>
  props.busy ? '任务运行或保存中，请完成后重启更新' : '安装已下载的更新并重启应用',
);
function finiteBytes(value: number) {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}
function formatBytes(value: number) {
  const bytes = finiteBytes(value);
  return bytes >= 1024 * 1024
    ? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    : `${Math.round(bytes)} B`;
}
const downloadStatus = computed(() => {
  const { downloaded, total } = progress.value;
  const bytes = formatBytes(downloaded);
  if (total === null || !Number.isFinite(total) || total <= 0) return `下载中 · ${bytes}`;
  const percent = Math.min(100, Math.floor((finiteBytes(downloaded) / total) * 100));
  return percent >= 100 ? '下载 100% · 正在校验…' : `下载中 ${percent}% · ${bytes}`;
});
const status = computed(() => {
  if (phase.value === 'checking') return '正在检查更新…';
  if (phase.value === 'downloading') return downloadStatus.value;
  if (phase.value === 'ready')
    return props.busy ? '下载完成，任务或保存结束后可重启' : '下载完成，可重启更新';
  if (phase.value === 'installing') return '正在安装，即将重启…';
  if (update.value) return `发现新版本 v${update.value.version}`;
  return '';
});
function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
function clearTimer() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
}
function scheduleCheck() {
  clearTimer();
  if (desktop && !disposed && !transferActive.value) {
    timer = setTimeout(() => void checkUpdate(), CHECK_INTERVAL);
  }
}
async function checkUpdate() {
  if (disposed || checkDisabled.value) return;
  clearTimer();
  phase.value = 'checking';
  checkError.value = '';
  try {
    const result = await invoke<UpdateInfo | null>('check_update');
    if (disposed) return;
    update.value = result;
    phase.value = result?.downloaded ? 'ready' : result ? 'available' : 'idle';
  } catch (error) {
    if (disposed) return;
    checkError.value = errorText(error);
    phase.value = update.value ? 'available' : 'idle';
  } finally {
    if (!disposed) scheduleCheck();
  }
}
function detachChannel() {
  // Rust 释放 Channel 时由 Tauri 清理 IPC 回调；提前移除组件回调，避免继续持有视图状态。
  if (channel) channel.onmessage = () => {};
  channel = undefined;
}
async function downloadUpdate() {
  if (!desktop || disposed || phase.value !== 'available') return;
  clearTimer();
  phase.value = 'downloading';
  actionError.value = '';
  progress.value = { downloaded: 0, total: null };
  try {
    channel = new Channel<DownloadProgress>();
    channel.onmessage = (value) => {
      if (!disposed && phase.value === 'downloading') progress.value = value;
    };
    await invoke('download_update', { onProgress: channel });
    // 字节下载完成后仍需等待 Rust 校验签名成功，才能提供安装入口。
    if (!disposed) phase.value = 'ready';
  } catch (error) {
    if (disposed) return;
    phase.value = 'available';
    actionError.value = `下载失败：${errorText(error)}。请重试下载。`;
    scheduleCheck();
  } finally {
    detachChannel();
  }
}
async function installUpdate() {
  // 按钮禁用之外再校验一次，防止双击或任务状态变化触发安装；Rust 仍做最终检查。
  if (!desktop || disposed || props.busy || phase.value !== 'ready') return;
  phase.value = 'installing';
  actionError.value = '';
  try {
    await invoke('install_update');
  } catch (error) {
    if (disposed) return;
    phase.value = 'ready';
    actionError.value = `安装失败：${errorText(error)}。请点击重启更新重试。`;
  }
}
onMounted(() => {
  if (desktop) void checkUpdate();
});
onUnmounted(() => {
  disposed = true;
  clearTimer();
  detachChannel();
});
</script>

<template>
  <div class="app-updater">
    <div class="app-updater-row">
      <button
        type="button"
        class="app-updater-version"
        :title="versionTitle"
        :aria-label="versionTitle"
        :disabled="checkDisabled"
        @click="checkUpdate"
      >
        v{{ version }}
      </button>
      <button
        v-if="update"
        type="button"
        class="app-updater-action"
        :disabled="
          phase === 'downloading' ||
          phase === 'installing' ||
          phase === 'checking' ||
          (phase === 'ready' && busy)
        "
        :title="
          phase === 'ready'
            ? installTitle
            : `更新至 v${update.version}${update.notes ? `：${update.notes}` : ''}`
        "
        :aria-label="phase === 'ready' ? installTitle : `更新至 v${update.version}`"
        @click="phase === 'ready' ? installUpdate() : downloadUpdate()"
      >
        {{
          phase === 'downloading'
            ? '下载中…'
            : phase === 'installing'
              ? '安装中…'
              : phase === 'ready'
                ? '重启更新'
                : actionError
                  ? '重试下载'
                  : '更新'
        }}
      </button>
    </div>
    <p v-if="status" class="app-updater-status" role="status" aria-live="polite">{{ status }}</p>
    <p v-if="actionError" class="app-updater-error" role="alert">{{ actionError }}</p>
  </div>
</template>

<style scoped>
.app-updater {
  min-width: 0;
  color: var(--sidebar-muted);
  font-size: 11px;
}
.app-updater-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.app-updater-version,
.app-updater-action {
  min-height: 28px;
  padding: 3px 7px;
  border: 1px solid transparent;
  border-radius: 5px;
  background: transparent;
  color: var(--sidebar-muted);
  font: inherit;
  cursor: pointer;
}
.app-updater-action {
  flex-shrink: 0;
  border-color: var(--sidebar-border);
  background: var(--sidebar-hover);
  color: var(--sidebar-accent);
}
.app-updater-version:hover:not(:disabled),
.app-updater-action:hover:not(:disabled) {
  background: var(--sidebar-selected);
  color: var(--sidebar-text);
}
.app-updater-version:focus-visible,
.app-updater-action:focus-visible {
  outline: 2px solid var(--sidebar-accent);
  outline-offset: 2px;
}
.app-updater-version:disabled,
.app-updater-action:disabled {
  cursor: default;
  opacity: 0.65;
}
.app-updater-status,
.app-updater-error {
  margin: 4px 0 0;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
.app-updater-error {
  color: var(--sidebar-accent);
}
</style>
