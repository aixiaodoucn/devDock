<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { api } from '../api';
import type { SqlpadExecutionRecord } from '../types';

const records = ref<SqlpadExecutionRecord[]>([]);
const loading = ref(false);
const failed = ref(false);
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
      <h3>SQLPad 执行记录</h3>
      <button class="button small" type="button" :disabled="loading" @click="refresh">
        刷新记录
      </button>
    </header>
    <p>本机保留最近 100 次 SQLPad 查询的执行记录，包含实际 SQL 和返回的业务数据。</p>
    <p v-if="failed" role="alert">读取执行记录失败，请刷新重试。</p>
    <p v-else-if="loading" role="status">正在读取执行记录…</p>
    <p v-else-if="!records.length" role="status">暂无执行记录。</p>
    <details v-for="record in records" :key="record.id">
      <summary>
        {{ new Date(record.startedAt).toLocaleString() }} · {{ record.connection }} ·
        {{ record.tool }} · {{ record.result !== null ? '已返回结果' : '未取得结果' }}
      </summary>
      <h4>执行 SQL</h4>
      <pre>{{ record.sql }}</pre>
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
    </details>
  </section>
</template>

<style scoped>
.sqlpad-history header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.sqlpad-history details {
  padding: 12px 0;
  border-top: 1px solid var(--line);
}
.sqlpad-history summary {
  cursor: pointer;
  overflow-wrap: anywhere;
}
.sqlpad-history pre {
  max-height: 24rem;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
