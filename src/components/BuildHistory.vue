<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { History } from 'lucide-vue-next';
import { api, desktop } from '../api';
import type { BuildHistorySettings, BuildRecord } from '../types';

const props = defineProps<{ projectId: string }>();
const DISPLAY_LIMIT_KEY = 'dev-dock-history-display-limit';
const DISPLAY_LIMIT_OPTIONS = [10, 20, 50, 100, 200, 500, 1000];
const DEFAULT_DISPLAY_LIMIT = 20;
function readDisplayLimit() {
  try {
    const saved = localStorage.getItem(DISPLAY_LIMIT_KEY);
    return (
      DISPLAY_LIMIT_OPTIONS.find((option) => String(option) === saved) ?? DEFAULT_DISPLAY_LIMIT
    );
  } catch {
    return DEFAULT_DISPLAY_LIMIT;
  }
}
const limit = ref(readDisplayLimit());
const preferenceError = ref('');
const records = ref<BuildRecord[]>([]);
// 展示偏好仅截断前端列表，不改变历史记录及后端保留规则。
const visibleRecords = computed(() => records.value.slice(0, limit.value));
const loading = ref(false);
const deleting = ref(false);
const opening = ref<string | null>(null);
const expanded = ref<string | null>(null);
const confirming = ref<string | null>(null);
const error = ref('');
const settings = ref<BuildHistorySettings | null>(null);
const settingsLoading = ref(false);
const saving = ref(false);
const editing = ref(false);
const retentionDraft = ref<string | number>('');
const settingsError = ref('');
const settingsNotice = ref('');
const confirmingReduction = ref(false);
const busy = computed(
  () => loading.value || deleting.value || settingsLoading.value || saving.value,
);
const retentionValue = computed(() => Number(retentionDraft.value));
const retentionInvalid = computed(
  () =>
    !String(retentionDraft.value).trim() ||
    !Number.isInteger(retentionValue.value) ||
    retentionValue.value < 1 ||
    retentionValue.value > 1000,
);
const statusLabels = {
  running: '构建中',
  success: '成功',
  error: '失败',
  cancelled: '已取消',
  interrupted: '已中断',
};
const modeLabels = { apk: 'Android APK', 'wgt-android': 'Android WGT', 'wgt-ios': 'iOS WGT' };
let generation = 0;
let disposed = false;
let refreshPending = false;
let settingsPending = false;
let unlisten: UnlistenFn | undefined;
const errorText = (value: unknown) => (value instanceof Error ? value.message : String(value));
function duration(value: number | null) {
  if (value === null) return '耗时未知';
  const seconds = Math.floor(Math.max(0, value) / 1000);
  return seconds < 60 ? `${seconds}秒` : `${Math.floor(seconds / 60)}分${seconds % 60}秒`;
}
function current(request: number) {
  return !disposed && request === generation;
}
async function refresh() {
  if (!desktop || disposed || !props.projectId) return;
  // 事件与删除并发时合并一次补读，避免旧快照恢复已经删除的记录。
  if (loading.value || deleting.value || saving.value) {
    refreshPending = true;
    return;
  }
  const request = generation;
  const projectId = props.projectId;
  loading.value = true;
  error.value = '';
  try {
    const result = await api.buildHistory(projectId);
    if (current(request)) {
      records.value = result;
      if (!result.some((record) => record.id === expanded.value)) expanded.value = null;
      confirming.value = null;
    }
  } catch (value) {
    if (current(request)) error.value = `历史读取失败：${errorText(value)}。请点击刷新重试。`;
  } finally {
    if (current(request)) {
      loading.value = false;
      flushRefresh();
    }
  }
}
function flushRefresh() {
  if (disposed || busy.value) return;
  if (settingsPending) {
    settingsPending = false;
    void refreshSettings();
  }
  if (refreshPending) {
    refreshPending = false;
    void refresh();
  }
}
async function refreshSettings() {
  if (!desktop || disposed) return;
  if (settingsLoading.value || saving.value || deleting.value) {
    settingsPending = true;
    return;
  }
  const request = generation;
  settingsLoading.value = true;
  settingsError.value = '';
  try {
    const result = await api.buildHistorySettings();
    if (current(request)) {
      const firstLoad = settings.value === null;
      settings.value = result;
      if (editing.value && firstLoad) retentionDraft.value = String(result.retentionLimit);
    }
  } catch (value) {
    if (current(request)) settingsError.value = `保留设置读取失败：${errorText(value)}。请重试。`;
  } finally {
    settingsLoading.value = false;
    flushRefresh();
  }
}
function openSettings() {
  if (!desktop || busy.value) return;
  retentionDraft.value = settings.value ? String(settings.value.retentionLimit) : '';
  confirmingReduction.value = false;
  settingsNotice.value = '';
  editing.value = true;
}
function cancelSettings() {
  if (busy.value) return;
  editing.value = false;
  confirmingReduction.value = false;
}
async function saveSettings(confirmed = false) {
  if (
    !desktop ||
    disposed ||
    busy.value ||
    !settings.value ||
    settingsError.value ||
    retentionInvalid.value
  )
    return;
  if (
    retentionValue.value < settings.value.retentionLimit &&
    (!confirmed || !confirmingReduction.value)
  ) {
    confirmingReduction.value = true;
    return;
  }
  const request = generation;
  const value = retentionValue.value;
  saving.value = true;
  settingsNotice.value = '';
  try {
    const result = await api.saveBuildHistorySettings(value);
    if (current(request)) {
      settings.value = { retentionLimit: result.retentionLimit };
      editing.value = false;
      confirmingReduction.value = false;
      settingsNotice.value = `保留设置已保存，已移除 ${result.removedCount} 条已结束记录；实际文件保留。`;
    }
    refreshPending = true;
  } catch (value) {
    if (current(request))
      settingsNotice.value = `保留设置保存失败：${errorText(value)}。草稿已保留，请重试。`;
  } finally {
    saving.value = false;
    flushRefresh();
  }
}
watch([retentionDraft, settings], () => {
  confirmingReduction.value = false;
});
async function remove(record: BuildRecord) {
  if (
    !desktop ||
    disposed ||
    busy.value ||
    record.status === 'running' ||
    confirming.value !== record.id
  )
    return;
  const request = generation;
  const projectId = props.projectId;
  deleting.value = true;
  error.value = '';
  try {
    await api.deleteBuildHistory(projectId, record.id);
    if (current(request)) {
      records.value = records.value.filter((item) => item.id !== record.id);
      confirming.value = null;
      if (expanded.value === record.id) expanded.value = null;
    }
  } catch (value) {
    if (current(request)) error.value = `移除记录失败：${errorText(value)}。请重试。`;
  } finally {
    // 删除跨项目切换仍持有锁；结果只更新发起操作的项目。
    deleting.value = false;
    flushRefresh();
  }
}
async function openArtifact(path: string, exists: boolean) {
  if (!desktop || disposed || !exists || opening.value) return;
  const request = generation;
  opening.value = path;
  error.value = '';
  try {
    await api.openPath(path);
  } catch (value) {
    if (current(request)) error.value = `定位产物失败：${errorText(value)}。请刷新后重试。`;
  } finally {
    if (current(request)) opening.value = null;
  }
}
watch(limit, (value) => {
  preferenceError.value = '';
  try {
    localStorage.setItem(DISPLAY_LIMIT_KEY, String(value));
  } catch {
    preferenceError.value = '本机展示偏好未保存，本次设置已生效；重新打开后可能恢复默认。';
  }
});
watch(visibleRecords, (visible) => {
  // 条目被展示上限或刷新结果隐藏后，清理详情及移除确认，避免再次出现残留状态。
  if (!visible.some((record) => record.id === expanded.value)) expanded.value = null;
  if (!visible.some((record) => record.id === confirming.value)) confirming.value = null;
});
watch(
  () => props.projectId,
  () => {
    generation++;
    records.value = [];
    expanded.value = confirming.value = opening.value = null;
    loading.value = refreshPending = false;
    editing.value = confirmingReduction.value = false;
    settingsNotice.value = '';
    error.value = '';
    void refreshSettings();
    void refresh();
  },
  { immediate: true },
);
onMounted(async () => {
  if (!desktop) return;
  try {
    const release = await listen<string>('build-history-updated', (event) => {
      if (disposed) return;
      // 空项目 ID 表示全局保留策略已更新，设置与当前项目记录一起补读。
      if (event.payload === '') {
        void refreshSettings();
        void refresh();
      } else if (event.payload === props.projectId) void refresh();
    });
    // Tauri 注册为异步操作，卸载后才拿到句柄也必须清理。
    if (disposed) release();
    else unlisten = release;
  } catch (value) {
    if (!disposed) error.value = `历史更新监听失败：${errorText(value)}。可点击刷新读取历史。`;
  }
});
onUnmounted(() => {
  disposed = true;
  generation++;
  unlisten?.();
});
</script>

<template>
  <section class="build-history" aria-label="构建历史" :aria-busy="busy">
    <header>
      <div class="history-heading">
        <h3><History :size="16" aria-hidden="true" />构建历史</h3>
        <span class="history-muted"
          >显示 {{ visibleRecords.length }} / 共 {{ records.length }} 条</span
        >
      </div>
      <div class="history-controls">
        <label class="history-limit">
          最多显示
          <select v-model.number="limit" aria-label="最多显示">
            <option v-for="option in DISPLAY_LIMIT_OPTIONS" :key="option" :value="option">
              {{ option }}条
            </option>
          </select>
        </label>
        <button
          class="button small"
          :disabled="!desktop || busy"
          :aria-expanded="editing"
          @click="openSettings"
        >
          保留设置
        </button>
        <button class="button small" :disabled="!desktop || busy" @click="refresh">
          {{ loading ? '读取中…' : '刷新' }}
        </button>
      </div>
    </header>
    <div v-if="editing" class="history-settings" role="group" aria-label="保留设置">
      <p class="history-muted">
        本机全局策略，作用于所有项目的已结束记录；运行中不清理，实际文件保留。
      </p>
      <div class="history-controls">
        <label class="history-retention">
          每项目保留上限
          <input
            v-model="retentionDraft"
            type="number"
            min="1"
            max="1000"
            step="1"
            aria-label="每项目保留上限"
            :disabled="busy || !settings || !!settingsError"
          />
        </label>
        <button class="button small" :disabled="busy" @click="cancelSettings">取消</button>
        <button
          class="button small"
          :disabled="busy || !settings || !!settingsError || retentionInvalid"
          @click="saveSettings()"
        >
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>
      <p v-if="retentionInvalid && settings" class="history-error" role="alert">
        请输入 1 到 1000 之间的整数。
      </p>
      <div
        v-if="confirmingReduction"
        class="history-confirm"
        role="group"
        aria-label="确认降低保留上限"
      >
        <p>
          每项目保留上限将从 {{ settings?.retentionLimit }} 降为
          {{
            retentionValue
          }}，立即清理所有项目超限的最旧已结束记录，无法恢复；运行中记录和实际文件保留。
        </p>
        <button class="button small danger" :disabled="busy" @click="saveSettings(true)">
          确认降低并保存
        </button>
      </div>
    </div>
    <div v-if="desktop && settingsError" class="history-error" role="alert">
      {{ settingsError }}
      <button class="button small" :disabled="busy" @click="refreshSettings">重试读取设置</button>
    </div>
    <p v-if="settingsNotice" class="history-muted" role="status">{{ settingsNotice }}</p>
    <p v-if="preferenceError" class="history-error" role="status">{{ preferenceError }}</p>
    <p v-if="!desktop" class="history-muted" role="status">
      请在桌面应用中查看本机构建历史，浏览器预览无法读取历史记录。
    </p>
    <template v-else>
      <p v-if="error" class="history-error" role="alert">{{ error }}</p>
      <p v-if="!records.length" class="history-muted" role="status">
        {{ loading ? '正在读取构建历史…' : error ? '暂时无法读取历史记录' : '暂无构建历史' }}
      </p>
      <ul v-else>
        <li v-for="record in visibleRecords" :key="record.id">
          <button
            class="history-row"
            :aria-expanded="expanded === record.id"
            @click="expanded = expanded === record.id ? null : record.id"
          >
            <span class="history-version"
              >{{ record.versionName || '未记录版本' }}
              <small>({{ record.versionCode || '—' }})</small></span
            >
            <span>{{ modeLabels[record.mode] }}</span
            ><span class="history-branch">{{ record.branch || '无分支信息' }}</span>
            <time :datetime="new Date(record.startedAt).toISOString()">{{
              new Date(record.startedAt).toLocaleString()
            }}</time>
            <span>{{ duration(record.durationMs) }}</span
            ><span class="history-status" :class="record.status">{{
              statusLabels[record.status]
            }}</span
            ><span>{{ expanded === record.id ? '收起' : '详情' }}</span>
          </button>
          <div v-if="expanded === record.id" class="history-details">
            <p class="history-summary">{{ record.summary || '未记录构建摘要' }}</p>
            <p class="history-muted">仅保存摘要与产物信息，不包含完整构建日志。</p>
            <div v-for="artifact in record.artifacts" :key="artifact.path" class="history-artifact">
              <code>{{ artifact.path }}</code
              ><span>{{ artifact.exists ? '文件存在' : '已缺失' }}</span
              ><button
                class="button small"
                :disabled="!artifact.exists || opening !== null"
                @click="openArtifact(artifact.path, artifact.exists)"
              >
                {{ opening === artifact.path ? '定位中…' : '定位' }}
              </button>
            </div>
            <p v-if="!record.artifacts.length" class="history-muted">没有记录产物路径</p>
            <div
              v-if="confirming === record.id"
              class="history-confirm"
              role="group"
              aria-label="确认移除记录"
            >
              <p>仅删除历史记录，不删除实际产物。确认移除这条记录？</p>
              <button class="button small" :disabled="busy" @click="confirming = null">取消</button
              ><button class="button small danger" :disabled="busy" @click="remove(record)">
                {{ deleting ? '移除中…' : '确认移除' }}
              </button>
            </div>
            <button
              v-else-if="record.status !== 'running'"
              class="button small"
              :disabled="busy"
              @click="confirming = record.id"
            >
              移除记录
            </button>
          </div>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.build-history {
  margin-top: 16px;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--paper);
  color: var(--text-secondary);
  font-size: 12px;
}
header,
h3,
.history-heading,
.history-controls,
.history-limit,
.history-retention {
  display: flex;
  align-items: center;
  gap: 8px;
}
header {
  flex-wrap: wrap;
  justify-content: space-between;
  margin-bottom: 12px;
}
.history-heading,
.history-controls {
  flex-wrap: wrap;
}
.history-limit,
.history-retention {
  flex-direction: row;
  white-space: nowrap;
  font-size: 11px;
}
.history-settings {
  display: grid;
  gap: 10px;
  margin-bottom: 12px;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
}
.history-limit select,
.history-retention input {
  width: 88px;
  height: var(--control-height-small);
  min-height: var(--control-height-small);
  padding: 4px 28px 4px 8px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background-color: var(--paper);
  color: var(--ink);
  font: inherit;
}
h3 {
  color: var(--ink);
}
ul {
  margin: 0;
  padding: 0;
  list-style: none;
}
li + li {
  border-top: 1px solid var(--line);
}
.history-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
  width: 100%;
  padding: 12px 0;
  border: 0;
  background: transparent;
  text-align: left;
  font-size: 11px;
}
.history-row:hover {
  color: var(--accent-text);
}
.history-version {
  color: var(--ink);
  font-weight: 600;
}
.history-version small {
  color: var(--muted);
  font-weight: 400;
}
.history-branch {
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.history-status {
  margin-left: auto;
  padding: 3px 6px;
  border-radius: 4px;
  background: var(--accent-soft);
  color: var(--accent-text);
}
.history-status.success {
  background: var(--success-soft);
  color: var(--success-text);
}
.history-status.error {
  background: var(--danger-soft);
  color: var(--danger-text);
}
.history-status.cancelled,
.history-status.interrupted {
  background: var(--warning-soft);
  color: var(--warning-text);
}
.history-details {
  display: grid;
  gap: 10px;
  padding: 0 0 14px;
}
.history-summary {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.6;
}
.history-muted {
  color: var(--muted);
  font-size: 11px;
  line-height: 1.6;
}
.history-error {
  color: var(--danger-text);
  margin-bottom: 10px;
  overflow-wrap: anywhere;
}
.history-artifact {
  display: flex;
  align-items: center;
  gap: 8px;
}
.history-artifact code {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
  font-size: 11px;
}
.history-artifact > span {
  color: var(--muted);
  font-size: 11px;
  flex-shrink: 0;
}
.history-details > button {
  justify-self: start;
}
.history-confirm {
  padding: 10px;
  border: 1px solid var(--danger-border);
  border-radius: 6px;
  background: var(--danger-surface);
}
.history-confirm p {
  margin-bottom: 8px;
}
.history-confirm button + button {
  margin-left: 8px;
}
</style>
