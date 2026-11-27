<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { api } from '../api';
import type { ScmPendingApproval } from '../types';

const FIELD_LABELS: Record<string, string> = {
  projectId: '目标项目 ID',
  scmProjectId: 'SCM 项目 ID',
  projectName: '项目名称',
  planId: '计划 ID',
  operation: 'Git 操作',
  push: '是否推送',
  keepTestPom: '保留测试 POM',
  expiresAtMs: '计划到期时间（毫秒）',
  name: '项目名称',
  branchSuffix: '分支后缀',
  projectType: '项目类型',
  user: '负责人',
  details: '项目说明',
  modules: '模块明细',
  moduleId: '目标模块 ID',
  authors: '负责人列表',
  revision: '修改基准版本',
  repositories: '仓库与分支',
};
const queue = ref<ScmPendingApproval[]>([]);
const now = ref(Date.now());
const valid = computed(() => queue.value.filter((entry) => entry.expiresAt > now.value));
const current = computed(() => valid.value[0]);
const seconds = computed(() =>
  current.value ? Math.ceil((current.value.expiresAt - now.value) / 1000) : 0,
);
const TOOL_LABELS: Record<string, string> = {
  scm_create_project: '创建 SCM 项目',
  scm_update_project: '修改 SCM 项目',
  scm_remove_project_module: '解除项目模块关联',
  backend_git_prepare: '准备 Git 计划',
  backend_git_execute: '执行 Git 计划',
};
const title = computed(() => TOOL_LABELS[current.value?.tool ?? ''] ?? '项目写入操作');
const fields = computed(() =>
  Object.entries(current.value?.arguments ?? {}).flatMap(([key, value]) =>
    key === 'input' && value && typeof value === 'object' && !Array.isArray(value)
      ? Object.entries(value)
      : [[key, value] as [string, unknown]],
  ),
);
const display = (value: unknown) =>
  typeof value === 'boolean'
    ? value
      ? '是'
      : '否'
    : typeof value === 'string'
      ? value || '（空）'
      : JSON.stringify(value, null, 2);
const dialog = ref<HTMLDialogElement>();
const deciding = ref(false);
const feedback = ref('');
let active = true;
let revision = 0;
let stop: UnlistenFn | undefined;
let tick: ReturnType<typeof setInterval> | undefined;
watch(dialog, (element) => element?.showModal(), { flush: 'post' });
watch(
  () => current.value?.requestId,
  () => {
    feedback.value = '';
  },
);
watch(current, (entry) => {
  clearInterval(tick);
  if (entry)
    tick = setInterval(() => {
      now.value = Date.now();
    }, 250);
});

async function refresh() {
  // 订阅只作唤醒：列表是宿主快照，较早发起的读取不能覆盖新请求。
  const version = ++revision;
  try {
    const pending = await api.listScmProjectApprovals();
    if (!active || version !== revision) return;
    now.value = Date.now();
    const id = current.value?.requestId;
    queue.value = pending
      .filter((entry) => entry.requestId === id)
      .concat(pending.filter((entry) => entry.requestId !== id));
  } catch {
    if (active && version === revision) queue.value = [];
  }
}
async function decide(event: MouseEvent, approved: boolean) {
  // 按钮绑定渲染时看到的 ID，旧弹窗的点击不能落到队列下一项。
  const id = (event.currentTarget as HTMLButtonElement).value;
  const entry = current.value;
  now.value = Date.now();
  if (!entry || entry.requestId !== id || entry.expiresAt <= now.value || deciding.value) return;
  deciding.value = true;
  try {
    await api.resolveScmProjectApproval(id, approved);
    queue.value = queue.value.filter((item) => item.requestId !== id);
  } catch {
    feedback.value = '决定未能确认，请重新核对当前请求。';
  } finally {
    if (active) await refresh();
    deciding.value = false;
  }
}
onMounted(async () => {
  try {
    const main = getCurrentWebviewWindow();
    if (main.label !== 'main') return;
    const unlisten = await main.listen('scm-project-approval-requested', refresh);
    if (!active) return unlisten();
    stop = unlisten;
    await refresh();
  } catch {
    queue.value = [];
  }
});
onUnmounted(() => {
  active = false;
  clearInterval(tick);
  stop?.();
});
</script>

<template>
  <dialog
    v-if="current"
    :key="current.requestId"
    ref="dialog"
    class="approval"
    aria-labelledby="scm-approval-title"
    @cancel.prevent
  >
    <header>
      <small>MCP 写入审批 · {{ valid.length }} 项待处理</small>
      <h2 id="scm-approval-title">{{ title }}</h2>
      <p v-if="current.tool === 'scm_remove_project_module'">
        只解除下列模块与项目的关联，不删除项目或仓库。
      </p>
      <p v-else>批准仅对本次请求有效。请先核对目标和下列全部变更。</p>
    </header>
    <dl>
      <dt>站点</dt>
      <dd>{{ current.site }}</dd>
      <dt>账号</dt>
      <dd>{{ current.account }}</dd>
      <dt>请求工具</dt>
      <dd>
        <code>{{ current.tool }}</code>
      </dd>
      <template v-if="current.tool === 'scm_create_project'"
        ><dt>目标项目 ID</dt>
        <dd>新项目（尚无 ID）</dd></template
      >
      <template v-for="[key, value] in fields" :key="key">
        <dt>{{ FIELD_LABELS[key] ?? key }}</dt>
        <dd v-if="Array.isArray(value) && value.length">
          <ul>
            <li v-for="(item, index) in value" :key="index">
              <template v-if="key === 'repositories' && item && typeof item === 'object'">
                <strong>{{
                  item.repository?.name ?? item.name ?? item.id ?? `仓库 ${index + 1}`
                }}</strong>
                <div v-if="item.currentBranch">
                  当前分支：<code>{{ item.currentBranch }}</code>
                </div>
                <div v-if="item.targetBranch">
                  目标分支：<code>{{ item.targetBranch }}</code>
                </div>
                <div v-if="(item.repository ?? item).developmentBranch">
                  开发分支：<code>{{ (item.repository ?? item).developmentBranch }}</code>
                </div>
                <div v-if="(item.repository ?? item).testBranch">
                  测试分支：<code>{{ (item.repository ?? item).testBranch }}</code>
                </div>
                <details>
                  <summary>完整仓库快照</summary>
                  {{ display(item) }}
                </details>
              </template>
              <template v-else>{{ display(item) }}</template>
            </li>
          </ul>
        </dd>
        <dd v-else>{{ Array.isArray(value) ? '无' : display(value) }}</dd>
      </template>
    </dl>
    <p v-if="feedback" role="alert">{{ feedback }}</p>
    <footer>
      <span role="timer">{{ seconds }} 秒后过期</span>
      <button
        type="button"
        autofocus
        :disabled="deciding"
        :value="current.requestId"
        @click="decide($event, false)"
      >
        拒绝
      </button>
      <button
        type="button"
        class="primary"
        :disabled="deciding"
        :value="current.requestId"
        @click="decide($event, true)"
      >
        批准本次操作
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.approval {
  width: min(610px, 90vw);
  max-height: 88vh;
  padding: 0;
  color: var(--ink);
  background: var(--paper);
  border: 1px solid var(--line);
  border-top: 4px solid var(--danger);
  border-radius: 12px;
}
.approval::backdrop {
  background: rgb(23 25 36 / 52%);
}
header,
dl,
footer,
[role='alert'] {
  padding: 14px 24px;
  margin: 0;
}
h2 {
  font-size: 21px;
  margin: 4px 0;
}
small,
p,
dt {
  color: var(--text-secondary);
  font-size: 13px;
}
header p {
  margin: 0;
}
dl {
  display: grid;
  grid-template-columns: minmax(6em, auto) minmax(0, 1fr);
  gap: 8px 16px;
  overflow: auto;
  max-height: 48vh;
  border-block: 1px solid var(--line);
  background: var(--surface-subtle);
}
dd {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: 13px;
}
ul {
  padding-inline-start: 1.3em;
  margin: 0;
}
li + li {
  margin-top: 4px;
}
footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
[role='timer'] {
  margin-inline-end: auto;
  font-variant-numeric: tabular-nums;
  color: var(--danger);
}
</style>
