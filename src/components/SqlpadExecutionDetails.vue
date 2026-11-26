<script setup lang="ts">
import { ref, shallowRef } from 'vue';
import { api } from '../api';
import type { SqlpadExecutionDetail, SqlpadExecutionSummary } from '../types';

const props = defineProps<{ record: SqlpadExecutionSummary }>();
const detail = shallowRef<SqlpadExecutionDetail>();
const loading = ref(false);
const failed = ref(false);

async function loadDetail() {
  if (loading.value || detail.value) return;
  loading.value = true;
  failed.value = false;
  try {
    detail.value = await api.sqlpadExecutionDetail(props.record.id);
  } catch {
    failed.value = true;
  } finally {
    loading.value = false;
  }
}
function onToggle(event: Event) {
  if ((event.currentTarget as HTMLDetailsElement).open) void loadDetail();
}
</script>

<template>
  <details @toggle="onToggle">
    <summary>
      <span class="record-title">
        <strong>{{ record.connection }}</strong>
        <span class="record-status" :class="{ returned: record.hasResult }">{{
          record.hasResult ? '已返回结果' : '未取得结果'
        }}</span>
      </span>
      <span class="record-meta"
        >{{ new Date(record.startedAt).toLocaleString() }} · <code>{{ record.tool }}</code></span
      >
    </summary>
    <p v-if="loading" role="status">正在读取详情…</p>
    <div v-else-if="failed" role="alert">
      读取详情失败。<button type="button" class="button small" @click="loadDetail">重试</button>
    </div>
    <div v-else-if="detail" class="record-body">
      <h4>执行 SQL</h4>
      <pre><code>{{ detail.sql }}</code></pre>
      <template v-if="detail.error !== null"
        ><h4>错误信息</h4>
        <pre>{{ detail.error }}</pre>
      </template>
      <template v-else
        ><h4>查询结果</h4>
        <pre>{{
          detail.result === null
            ? '无结果'
            : typeof detail.result === 'string'
              ? detail.result
              : JSON.stringify(detail.result, null, 2)
        }}</pre>
      </template>
    </div>
  </details>
</template>

<style scoped>
.record-meta {
  color: var(--muted);
  font-size: 12px;
}
summary {
  display: grid;
  grid-template-columns: 12px minmax(0, 1fr);
  gap: 3px 8px;
  align-items: baseline;
  cursor: pointer;
  overflow-wrap: anywhere;
  list-style: none;
}
summary::-webkit-details-marker {
  display: none;
}
summary::before {
  content: '';
  width: 6px;
  height: 6px;
  border: solid var(--muted);
  border-width: 0 1.5px 1.5px 0;
  transform: rotate(-45deg);
}
details[open] summary::before {
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
pre {
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
