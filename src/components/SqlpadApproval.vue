<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { api } from '../api';
import type { SqlpadPendingApproval } from '../types';

const pending = ref<SqlpadPendingApproval | null>(null);
const dialog = ref<HTMLDialogElement>();
const deciding = ref(false);
let active = true;
let revision = 0;
let unlisten: UnlistenFn | undefined;
let expiry: ReturnType<typeof setTimeout> | undefined;

watch(dialog, (element) => element?.showModal(), { flush: 'post' });

async function refresh() {
  // 事件只是提示。丢弃晚到的快照，避免重新显示已取消或过期的审批。
  const version = ++revision;
  clearTimeout(expiry);
  pending.value = null;
  try {
    const current = await api.sqlpadPendingApproval();
    if (!active || version !== revision || !current || current.expiresAt <= Date.now()) return;
    pending.value = current;
    expiry = setTimeout(() => (pending.value = null), current.expiresAt - Date.now());
  } catch {
    // 读取失败时没有可审批的快照，等待下一次通知或窗口重挂。
  }
}

async function decide(requestId: string, approved: boolean) {
  const current = pending.value;
  if (!current || deciding.value || current.requestId !== requestId) return;
  if (current.expiresAt <= Date.now()) {
    pending.value = null;
    return;
  }
  deciding.value = true;
  try {
    await api.sqlpadDecideApproval(requestId, approved);
  } catch {
    // 请求可能已取消；只根据后端当前状态恢复操作，不重试决策。
  } finally {
    if (active) await refresh();
    deciding.value = false;
  }
}

onMounted(async () => {
  try {
    const main = getCurrentWebviewWindow();
    if (main.label !== 'main') return;
    // 先订阅再读取，以覆盖窗口重挂期间到达的审批。
    const stop = await main.listen('sqlpad-approval-changed', refresh);
    if (!active) return stop();
    unlisten = stop;
    await refresh();
  } catch {
    pending.value = null;
  }
});
onUnmounted(() => {
  active = false;
  clearTimeout(expiry);
  unlisten?.();
});
</script>

<template>
  <dialog
    v-if="pending"
    :key="pending.requestId"
    ref="dialog"
    class="approval"
    aria-labelledby="sqlpad-approval-title"
    aria-describedby="sqlpad-approval-warning"
    @cancel.prevent="decide(pending.requestId, false)"
  >
    <h2 id="sqlpad-approval-title">批准本次生产查询？</h2>
    <p id="sqlpad-approval-warning">批准后，本次查询返回的生产数据将发送给 AI。</p>
    <dl>
      <dt>SQLPad 连接</dt>
      <dd>{{ pending.connection }}</dd>
      <dt>SQL 或元数据请求</dt>
      <dd>
        <pre tabindex="0" dir="ltr">{{ pending.preview }}</pre>
      </dd>
    </dl>
    <footer>
      <button
        type="button"
        autofocus
        :disabled="deciding"
        @click="decide(pending.requestId, false)"
      >
        拒绝
      </button>
      <button
        type="button"
        class="primary"
        :disabled="deciding"
        @click="decide(pending.requestId, true)"
      >
        批准本次查询
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.approval {
  width: min(580px, 90vw);
  max-height: 88vh;
  padding: 24px;
  border: 1px solid var(--line);
  border-top: 4px solid var(--danger);
  border-radius: 12px;
  background: var(--paper);
  color: var(--ink);
}
.approval::backdrop {
  background: rgb(23 25 36 / 52%);
}
h2 {
  margin: 0;
  font-size: 20px;
}
p {
  color: var(--danger);
}
dt {
  margin-top: 16px;
  font-size: 13px;
  color: var(--text-secondary);
}
dd {
  margin: 6px 0 0;
  overflow-wrap: anywhere;
}
pre {
  margin: 0;
  padding: 12px;
  max-height: 38vh;
  overflow: auto;
  white-space: pre-wrap;
  font-family: var(--mono);
  background: var(--surface-subtle);
  border: 1px solid var(--line);
  border-radius: 6px;
}
footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}
</style>
