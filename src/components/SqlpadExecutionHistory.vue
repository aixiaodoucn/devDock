<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { api } from '../api';
import type { SqlpadExecutionRecord } from '../types';

const records = ref<SqlpadExecutionRecord[]>([]);
const loading = ref(false);
const failed = ref(false);
const rerunningId = ref<string | null>(null);
const rerunFailedId = ref<string | null>(null);

async function rerun(record: SqlpadExecutionRecord) {
  if (record.canRerun !== true || rerunningId.value !== null || loading.value) return;
  rerunningId.value = record.id;
  rerunFailedId.value = null;
  try {
    await api.sqlpadRerunExecution(record.id);
  } catch {
    rerunFailedId.value = record.id;
  } finally {
    // 执行失败也可能留下记录，列表顺序以桌面端返回的数据为准。
    await refresh();
    rerunningId.value = null;
  }
}

async function refresh() {
  if (loading.value) return;
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
onMounted(refresh);
</script>

<template>
  <section class="panel sqlpad-history" aria-label="SQLPad 执行记录" :aria-busy="loading">
    <header>
      <div>
        <h3>SQLPad 执行记录</h3>
        <p class="history-note">本机保留最近 100 次查询的实际 SQL 和返回的业务数据。</p>
      </div>
      <button
        class="button small"
        type="button"
        :disabled="loading || rerunningId !== null"
        @click="refresh"
      >
        刷新记录
      </button>
    </header>
    <p v-if="failed" role="alert">读取执行记录失败，请刷新重试。</p>
    <p v-else-if="loading" role="status">正在读取执行记录…</p>
    <p v-else-if="!records.length" role="status">暂无执行记录。</p>
    <div v-for="record in records" :key="record.id" class="history-row">
      <details>
        <summary>
          <span class="record-title">
            <strong>{{ record.connection }}</strong>
            <span class="record-status" :class="{ returned: record.result !== null }">{{
              record.result !== null ? '已返回结果' : '未取得结果'
            }}</span>
          </span>
          <span class="record-meta"
            >{{ new Date(record.startedAt).toLocaleString() }} ·
            <code>{{ record.tool }}</code></span
          >
        </summary>
        <div class="record-body">
          <h4>执行 SQL</h4>
          <pre><code>{{ record.sql }}</code></pre>
          <template v-if="record.error !== null"
            ><h4>错误信息</h4>
            <pre>{{ record.error }}</pre>
          </template>
          <template v-else
            ><h4>查询结果</h4>
            <pre>{{
              record.result === null
                ? '无结果'
                : typeof record.result === 'string'
                  ? record.result
                  : JSON.stringify(record.result, null, 2)
            }}</pre>
          </template>
        </div>
      </details>
      <button
        class="button small"
        type="button"
        :disabled="record.canRerun !== true || loading || rerunningId !== null"
        :aria-describedby="record.canRerun !== true ? `rerun-reason-${record.id}` : undefined"
        @click="rerun(record)"
      >
        {{ rerunningId === record.id ? '重新执行中…' : '重新执行' }}
      </button>
      <small v-if="record.canRerun !== true" :id="`rerun-reason-${record.id}`"
        >此记录缺少安全重新执行所需的信息。</small
      >
      <small v-if="rerunFailedId === record.id" role="alert"
        >重新执行失败，请查看执行记录后重试。</small
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
.record-meta,
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
.history-row small {
  grid-column: 1 / -1;
}
.sqlpad-history summary {
  display: grid;
  grid-template-columns: 12px minmax(0, 1fr);
  gap: 3px 8px;
  align-items: baseline;
  cursor: pointer;
  overflow-wrap: anywhere;
  list-style: none;
}
.sqlpad-history summary::-webkit-details-marker {
  display: none;
}
.sqlpad-history summary::before {
  content: '';
  width: 6px;
  height: 6px;
  border: solid var(--muted);
  border-width: 0 1.5px 1.5px 0;
  transform: rotate(-45deg);
}
.sqlpad-history details[open] summary::before {
  transform: rotate(45deg);
}
.record-title {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px 8px;
}
.record-meta {
  grid-column: 2;
}
.record-status {
  font-size: 12px;
  border-radius: 4px;
  padding: 2px 6px;
  background: var(--surface-subtle);
  color: var(--text-secondary);
}
.record-status.returned {
  background: var(--success-soft);
  color: var(--success-text);
}
.record-body {
  margin-top: 12px;
}
.record-body h4 {
  margin: 12px 0 5px;
  font-size: 12px;
  color: var(--text-secondary);
}
.sqlpad-history pre {
  margin: 0;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-subtle);
  max-height: 24rem;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
