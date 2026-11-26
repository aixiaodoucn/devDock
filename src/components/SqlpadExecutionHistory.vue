<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { api } from '../api';
import SqlpadExecutionDetails from './SqlpadExecutionDetails.vue';
import type { SqlpadExecutionSummary } from '../types';

const records = ref<SqlpadExecutionSummary[]>([]);
const loading = ref(false);
const failed = ref(false);
const rerunningId = ref<string | null>(null);

const limit = ref<number | null>(null);
const draft = ref<number | string>('');
const limitLoading = ref(false);
const limitError = ref(false);
const actionError = ref('');
const pending = ref<{ kind: 'delete'; id: string } | { kind: 'clear' } | null>(null);
const saving = ref(false);
const busy = computed(() => loading.value || saving.value || rerunningId.value !== null);
const validLimit = computed(
  () =>
    typeof draft.value === 'number' &&
    Number.isInteger(draft.value) &&
    draft.value >= 1 &&
    draft.value <= 10000,
);

async function loadLimit() {
  limitLoading.value = true;
  limitError.value = false;
  try {
    limit.value = draft.value = await api.sqlpadHistoryLimit();
  } catch {
    limitError.value = true;
  } finally {
    limitLoading.value = false;
  }
}

async function saveLimit() {
  if (busy.value || limit.value === null || !validLimit.value || draft.value === limit.value)
    return;
  saving.value = true;
  pending.value = null;
  actionError.value = '';
  try {
    limit.value = draft.value = await api.sqlpadSetHistoryLimit(Number(draft.value));
    await refresh();
  } catch {
    actionError.value = '保存保留上限失败，请重试。';
  } finally {
    saving.value = false;
  }
}

async function confirmRemoval() {
  const action = pending.value;
  if (busy.value || !action) return;
  saving.value = true;
  actionError.value = '';
  try {
    if (action.kind === 'delete') await api.sqlpadDeleteExecution(action.id);
    else await api.sqlpadClearHistory();
    await refresh();
  } catch {
    actionError.value =
      action.kind === 'delete' ? '删除执行记录失败，请重试。' : '清空执行记录失败，请重试。';
  } finally {
    pending.value = null;
    saving.value = false;
  }
}

async function rerun(record: SqlpadExecutionSummary) {
  if (record.canRerun !== true || busy.value) return;
  pending.value = null;
  actionError.value = '';
  rerunningId.value = record.id;
  try {
    await api.sqlpadRerunExecution(record.id);
  } catch {
    actionError.value = `「${record.connection}」重新执行失败，请查看执行记录后重试。`;
  } finally {
    // 执行失败也可能留下记录，列表顺序以桌面端返回的数据为准。
    await refresh();
    rerunningId.value = null;
  }
}

async function refresh() {
  if (loading.value) return;
  pending.value = null;
  loading.value = true;
  failed.value = false;
  try {
    records.value = await api.sqlpadExecutionHistory();
  } catch {
    // 读取失败时不显示旧连接的数据，也不泄露底层错误详情。
    records.value = [];
    failed.value = true;
  } finally {
    loading.value = false;
  }
}
onMounted(() => {
  void refresh();
  void loadLimit();
});
</script>

<template>
  <section class="panel sqlpad-history" aria-label="SQLPad 执行记录" :aria-busy="loading">
    <header>
      <div>
        <h3>SQLPad 执行记录</h3>
        <p class="history-note">本机保存查询的实际 SQL 和返回的业务数据。</p>
      </div>
      <button class="button small" type="button" :disabled="busy" @click="refresh">刷新记录</button>
    </header>
    <div class="history-actions retention">
      <label
        >最多保留
        <input
          v-model.number="draft"
          type="number"
          min="1"
          max="10000"
          step="1"
          aria-label="历史保留上限"
          aria-describedby="sqlpad-limit-note"
          :disabled="busy || limit === null"
        />
        条</label
      >
      <button
        type="button"
        class="button small"
        :disabled="busy || limit === null || !validLimit || draft === limit"
        @click="saveLimit"
      >
        保存上限
      </button>
      <button
        type="button"
        class="button small"
        :disabled="busy || !records.length"
        @click="pending = { kind: 'clear' }"
      >
        清空记录
      </button>
    </div>
    <p id="sqlpad-limit-note" class="history-note">
      可保留 1–10000 条，默认 100 条。降低上限并保存后会立即清理超出的最旧记录。
    </p>
    <p v-if="limitError" role="alert">
      读取保留上限失败。<button
        class="button small"
        type="button"
        :disabled="limitLoading"
        @click="loadLimit"
      >
        重试
      </button>
    </p>
    <p v-if="limit !== null && !validLimit" role="alert">请输入 1 到 10000 之间的整数。</p>
    <div
      v-if="pending?.kind === 'clear'"
      class="history-actions"
      role="group"
      aria-label="确认清空记录"
    >
      <span>清空全部执行记录后无法恢复。</span>
      <button type="button" class="button small" :disabled="busy" @click="pending = null">
        取消
      </button>
      <button type="button" class="button small danger" :disabled="busy" @click="confirmRemoval">
        确认清空
      </button>
    </div>
    <p v-if="actionError" role="alert">{{ actionError }}</p>
    <p v-if="failed" role="alert">读取执行记录失败，请刷新重试。</p>
    <p v-else-if="loading" role="status">正在读取执行记录…</p>
    <p v-else-if="!records.length" role="status">暂无执行记录。</p>
    <div v-for="record in records" :key="record.id" class="history-row">
      <SqlpadExecutionDetails :record="record" />
      <div class="history-actions">
        <button
          class="button small primary"
          type="button"
          :disabled="record.canRerun !== true || busy"
          :aria-describedby="record.canRerun !== true ? `rerun-reason-${record.id}` : undefined"
          @click="rerun(record)"
        >
          {{ rerunningId === record.id ? '重新执行中…' : '重新执行' }}
        </button>
        <button
          class="button small"
          type="button"
          :disabled="busy"
          @click="pending = { kind: 'delete', id: record.id }"
        >
          删除
        </button>
      </div>
      <div
        v-if="pending?.kind === 'delete' && pending.id === record.id"
        class="history-actions confirm-delete"
        role="group"
        aria-label="确认删除记录"
      >
        <span>删除这条执行记录后无法恢复。</span>
        <button type="button" class="button small" :disabled="busy" @click="pending = null">
          取消
        </button>
        <button type="button" class="button small danger" :disabled="busy" @click="confirmRemoval">
          确认删除
        </button>
      </div>
      <small v-if="record.canRerun !== true" :id="`rerun-reason-${record.id}`"
        >此记录缺少安全重新执行所需的信息。</small
      >
    </div>
  </section>
</template>

<style scoped>
.sqlpad-history header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.history-note,
.history-row small {
  color: var(--muted);
  font-size: 12px;
}
.sqlpad-history header h3 {
  margin: 0;
}
.history-note {
  margin: 4px 0 0;
}
.sqlpad-history button {
  white-space: nowrap;
}
.history-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 6px 12px;
  padding: 12px 0;
  border-top: 1px solid var(--line);
}
.confirm-delete,
.history-row small {
  grid-column: 1 / -1;
}
.history-actions {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.retention {
  margin-top: 12px;
}
.retention label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.retention input {
  width: 6em;
  height: var(--control-height-small);
  padding-block: 0;
}
#sqlpad-limit-note {
  margin-bottom: 12px;
}
</style>
