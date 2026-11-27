<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { api } from '../api';
import type { McpCatalogTool } from '../types';

const SOURCE_LABELS: Record<string, string> = {
  scm: 'SCM 日志',
  sqlpad: 'SQLPad 数据库',
  'scm-project': '项目 SCM',
  'backend-git': '后端项目 Git',
};
const ENVIRONMENT_LABELS: Record<string, string> = {
  production: '生产环境',
  project: '项目站点',
  local: '本机',
};
const tools = ref<McpCatalogTool[]>([]);
const loading = ref(true);
const failed = ref(false);
const listener = ref<'pending' | 'running' | 'stopped' | 'unknown'>('pending');
const listenerLabels = {
  pending: '正在检查本机监听…',
  running: '本机已监听',
  stopped: '本机未监听',
  unknown: '本机监听状态未知',
};
const scanScope = ref('读取中');
const rawPolicy = ref('读取中');
async function readPolicy() {
  const [scope, raw] = await Promise.allSettled([api.logMcpScanScope(), api.logMcpRawStatus()]);
  scanScope.value =
    scope.status === 'fulfilled' && (scope.value === 'first' || scope.value === 'all')
      ? scope.value === 'all'
        ? '全部实例'
        : '第一个实例'
      : '未知';
  rawPolicy.value =
    raw.status === 'fulfilled' && typeof raw.value === 'boolean'
      ? raw.value
        ? '已开启'
        : '未开启'
      : '未知';
}
const feedback = ref({ tool: '', message: '' });
const groups = computed(() => {
  const bySource = new Map<string, McpCatalogTool[]>();
  for (const tool of tools.value) {
    const group = bySource.get(tool.source);
    if (group) group.push(tool);
    else bySource.set(tool.source, [tool]);
  }
  return bySource;
});
async function loadCatalog() {
  loading.value = true;
  failed.value = false;
  try {
    tools.value = await api.mcpToolCatalog();
  } catch {
    failed.value = true;
  } finally {
    loading.value = false;
  }
}
async function readListener() {
  listener.value = 'pending';
  try {
    listener.value = (await api.logMcpStatus()).enabled ? 'running' : 'stopped';
  } catch {
    listener.value = 'unknown';
  }
}
async function copy(tool: McpCatalogTool, field: 'name' | 'example') {
  try {
    await navigator.clipboard.writeText(tool[field]);
    feedback.value = {
      tool: tool.name,
      message: field === 'name' ? '已复制工具名' : '已复制提问示例',
    };
  } catch {
    feedback.value = { tool: tool.name, message: '复制失败，请选中文字手动复制。' };
  }
}
onMounted(() => {
  // 目录独立于服务监听状态，停用或状态读取失败时仍可浏览。
  void loadCatalog();
  void readListener();
  void readPolicy();
});
</script>

<template>
  <section class="catalog" aria-label="MCP 工具目录">
    <header>
      <div class="heading">
        <h2>MCP 工具目录</h2>
        <span class="listener" role="status" :class="{ running: listener === 'running' }">{{
          listenerLabels[listener]
        }}</span
        ><button
          v-if="listener === 'unknown'"
          type="button"
          class="button small"
          @click="readListener"
        >
          重试状态
        </button>
      </div>
      <p>状态仅反映本机服务监听情况，账号登录和客户端连通性需另行确认。</p>
    </header>
    <p v-if="loading" role="status">正在加载工具目录…</p>
    <div v-else-if="failed" role="alert" class="heading">
      工具目录加载失败。<button class="button small" type="button" @click="loadCatalog">
        重试加载
      </button>
    </div>
    <p v-else-if="!tools.length">暂无工具</p>
    <section v-for="[source, entries] in groups" :key="source" class="group">
      <h3>
        {{ SOURCE_LABELS[source] ?? source }}
        <span>{{ entries.length }} 个工具</span>
      </h3>
      <details v-for="tool in entries" :key="tool.name">
        <summary>
          <strong>{{ tool.title }}</strong> <code>{{ tool.name }}</code>
          <span class="environment">{{
            ENVIRONMENT_LABELS[tool.environment] ?? tool.environment
          }}</span>
        </summary>
        <div class="detail">
          <p>{{ tool.description }}</p>
          <p v-if="source === 'scm'">
            当前日志策略：全文件扫描默认范围 · {{ scanScope }}；返回日志原文 · {{ rawPolicy }}。
          </p>
          <p v-else-if="source === 'scm-project'">
            使用「项目 SCM 登录」账号；创建、修改项目和解除模块关联需在主窗口逐次批准。
          </p>
          <p v-else-if="source === 'backend-git'">
            准备和执行 Git 计划需分别审批。批准前请核对项目、仓库、分支及是否推送。
          </p>
          <h4>参数</h4>
          <dl v-if="Object.keys(tool.inputSchema.properties).length">
            <div v-for="(param, key) in tool.inputSchema.properties" :key="key" class="parameter">
              <dt>
                <code>{{ key }}</code> <span>{{ param.type }}</span>
                <b>{{ tool.inputSchema.required.includes(key) ? '必填' : '可选' }}</b>
              </dt>
              <dd v-if="param.properties">
                <details>
                  <summary>对象字段</summary>
                  <pre>{{ JSON.stringify(param, null, 2) }}</pre>
                </details>
              </dd>
              <dd v-if="param.description">{{ param.description }}</dd>
              <dd v-if="param.default !== undefined">
                默认值 <code>{{ JSON.stringify(param.default) }}</code>
              </dd>
              <dd v-if="param.minimum !== undefined">最小值 {{ param.minimum }}</dd>
              <dd v-if="param.maximum !== undefined">最大值 {{ param.maximum }}</dd>
              <dd v-if="param.enum?.length">
                可选值 {{ param.enum.map((value) => JSON.stringify(value)).join('、') }}
              </dd>
            </div>
          </dl>
          <p v-else>无需参数</p>
          <h4>提问示例</h4>
          <blockquote>{{ tool.example }}</blockquote>
          <div class="actions">
            <button type="button" class="button small" @click="copy(tool, 'name')">
              复制工具名</button
            ><button type="button" class="button small" @click="copy(tool, 'example')">
              复制提问示例</button
            ><span role="status">{{ feedback.tool === tool.name ? feedback.message : '' }}</span>
          </div>
        </div>
      </details>
    </section>
  </section>
</template>

<style scoped>
.catalog {
  display: grid;
  gap: 14px;
  color: var(--ink);
  min-width: 0;
  overflow-wrap: anywhere;
}
.heading,
.actions,
dt {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
h2,
h3,
h4,
p,
dl,
dd,
blockquote {
  margin: 0;
}
h2 {
  font-size: 17px;
}
h3,
h4 {
  font-size: 13px;
}
.catalog p,
dd,
.actions span {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.6;
}
header p {
  margin-top: 4px;
}
.listener,
.environment,
dt span,
dt b,
h3 span {
  font-size: 11px;
  font-weight: 500;
  color: var(--muted);
}
.listener {
  padding: 2px 7px;
  border-radius: 4px;
  background: var(--surface-subtle);
}
.listener.running {
  background: var(--success-soft);
  color: var(--success-text);
}
.group {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--paper);
}
h3 {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--surface-subtle);
  border-radius: 8px 8px 0 0;
}
details {
  border-top: 1px solid var(--line);
}
summary {
  cursor: pointer;
  padding: 9px 12px;
  font-size: 13px;
}
summary::marker {
  color: var(--muted);
}
summary:hover,
details[open] > summary {
  background: var(--surface-subtle);
}
summary:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
summary code,
.environment {
  margin-inline-start: 6px;
}
code {
  font-size: 12px;
  color: var(--accent-text);
}
.detail pre {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.detail {
  display: grid;
  gap: 6px;
  padding: 6px 14px 12px;
}
h4 {
  margin-top: 4px;
}
.parameter {
  border-top: 1px solid var(--line);
  padding: 5px 0;
}
dd {
  display: inline-block;
  margin-right: 10px;
}
blockquote {
  padding: 6px 10px;
  border-inline-start: 2px solid var(--accent);
  background: var(--surface-subtle);
  font-size: 13px;
  white-space: pre-wrap;
}
</style>
