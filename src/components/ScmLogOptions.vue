<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue';
import { api, desktop, type LogMcpScanScope } from '../api';
const props = defineProps<{ serviceEnabled: boolean }>();
const pending = ref(false);
const scanScope = ref<LogMcpScanScope | null>(null);
const scopeBusy = ref(false);
const scopeError = ref('');
const rawEnabled = ref<boolean | null>(null);
const rawDialog = ref<HTMLDialogElement | null>(null);
const rawError = ref(false);
let active = true;

// 只展示后端确认的有效授权，读写失败时保留未知态，等待刷新。
async function syncRaw(enabled?: boolean) {
  rawEnabled.value = null;
  rawError.value = false;
  try {
    const result =
      enabled === undefined ? await api.logMcpRawStatus() : await api.logMcpSetRaw(enabled);
    if (typeof result !== 'boolean') throw new Error();
    if (active) rawEnabled.value = result;
  } catch {
    if (active) rawError.value = true;
  }
}

async function setRaw(enabled: boolean) {
  if (!desktop || !active || pending.value || !props.serviceEnabled || rawEnabled.value === null)
    return;
  rawDialog.value?.close();
  pending.value = true;
  try {
    await syncRaw(enabled);
  } finally {
    if (active) pending.value = false;
  }
}

async function syncScanScope(scope?: LogMcpScanScope) {
  if (scopeBusy.value || scope === scanScope.value) return;
  scopeBusy.value = true;
  scopeError.value = '';
  // 仅勾选后端已确认的范围，读写期间和失败后保持未选中。
  scanScope.value = null;
  try {
    const result = scope ? await api.logMcpSetScanScope(scope) : await api.logMcpScanScope();
    if (result !== 'first' && result !== 'all') throw new Error('查询范围未知');
    scanScope.value = result;
  } catch {
    scopeError.value = `${scope ? '更新' : '读取'}日志查询范围失败，请刷新状态。`;
  } finally {
    scopeBusy.value = false;
  }
}

onMounted(() => {
  void syncScanScope();
  if (props.serviceEnabled) void syncRaw();
});
onBeforeUnmount(() => {
  active = false;
});
</script>
<template>
  <div class="query-options" role="group" aria-label="SCM 日志查询选项">
    <fieldset :disabled="scopeBusy || scanScope === null">
      <legend>查询实例</legend>
      <div class="scope-picker">
        <label v-for="(label, scope) in { first: '第一个实例', all: '全部实例' }" :key="scope">
          <input
            type="radio"
            name="log-mcp-scan-scope"
            :value="scope"
            :checked="scanScope === scope"
            @click.prevent="syncScanScope(scope)"
          />
          <span>{{ label }}</span>
        </label>
      </div>
      <p class="hint">未指定实例时使用此范围。实例范围跨重启保留。</p>
      <p v-if="scopeError" class="error" role="alert">{{ scopeError }}</p>
    </fieldset>
    <div class="raw-policy">
      <div class="policy-heading">
        <strong>允许返回日志原文</strong
        ><button
          type="button"
          role="switch"
          aria-label="允许返回日志原文"
          :aria-checked="rawEnabled === true"
          aria-describedby="raw-policy-hint"
          class="raw-switch"
          :disabled="pending || !serviceEnabled || rawEnabled === null"
          @click="rawEnabled ? setRaw(false) : rawDialog?.showModal()"
        ></button>
      </div>
      <p class="hint" id="raw-policy-hint">
        原文授权仅对确认时的日志 SCM
        站点和账号有效。停用或退出会暂停服务，服务未启动时不会返回原文。
        在同一站点用相同账号重新启用服务可恢复授权。关闭开关会撤销已保存的授权。
      </p>
      <p v-if="rawError" class="error" role="alert">原文状态未知，请刷新状态。</p>
      <dialog
        ref="rawDialog"
        class="modal confirm-modal raw-confirm"
        aria-labelledby="raw-confirm-title"
        aria-describedby="raw-confirm-description"
      >
        <h2 id="raw-confirm-title">允许返回日志原文？</h2>
        <p id="raw-confirm-description">
          匹配行将不再脱敏，原文可能进入模型服务及历史记录。应用会为当前日志 SCM
          站点和账号记住此授权，可随时关闭。
        </p>
        <footer class="modal-footer">
          <button type="button" class="button" autofocus @click="rawDialog?.close()">取消</button>
          <button type="button" class="button primary" @click="setRaw(true)">确认开启</button>
        </footer>
      </dialog>
    </div>
  </div>
</template>
<style scoped>
.raw-confirm {
  max-width: calc(100vw - 32px);
  margin: auto;
  color: var(--ink);
  background: var(--paper);
  border-color: var(--line);
}
/* 共享 .modal 使用 flex；关闭的原生 dialog 必须保持隐藏。 */
.raw-confirm:not([open]) {
  display: none;
}
.raw-confirm::backdrop {
  background: color-mix(in srgb, var(--ink) 38%, transparent);
}
.raw-confirm > p {
  margin: 0;
  font-size: 14px;
  line-height: 1.65;
  color: var(--text-secondary);
}
.query-options {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  margin: 0;
  padding: 16px;
  background: var(--surface-subtle);
  border-block: 1px solid var(--line);
}
.query-options fieldset {
  display: block;
  min-width: 0;
  border: 0;
  padding: 0;
  margin: 0;
}
.query-options legend,
.policy-heading strong {
  font-size: 13px;
  font-weight: 650;
  padding: 0;
}
.scope-picker {
  display: inline-flex;
  border: 1px solid var(--line);
  border-radius: 7px;
  overflow: hidden;
  background: var(--paper);
  margin-top: 6px;
}
.scope-picker label {
  position: relative;
  display: block;
  cursor: pointer;
  margin: 0;
}
.scope-picker input {
  position: absolute;
  opacity: 0;
  width: 1px;
  height: 1px;
}
.scope-picker span {
  display: block;
  padding: 5px 12px;
  font-size: 13px;
}
.scope-picker input:checked + span {
  background: var(--accent-soft);
  color: var(--accent-text);
  font-weight: 650;
}
.scope-picker input:focus-visible + span {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.scope-picker input:disabled + span {
  opacity: 0.65;
  cursor: not-allowed;
}
.raw-policy {
  border-inline-start: 1px solid var(--line);
  padding-inline-start: 18px;
}
.policy-heading {
  display: flex;
  gap: 12px;
  align-items: center;
  justify-content: space-between;
}
.raw-switch {
  width: 32px;
  height: 19px;
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 2px;
  background: var(--line);
  cursor: pointer;
}
.raw-switch:disabled {
  opacity: 0.65;
  cursor: not-allowed;
}
.raw-switch[aria-checked='true'] {
  background: var(--accent);
}
.raw-switch[aria-checked='true']::before {
  transform: translateX(13px);
}
.raw-switch::before {
  content: '';
  display: block;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: var(--paper);
}
@container (max-width: 510px) {
  .query-options {
    grid-template-columns: minmax(0, 1fr);
  }
  .raw-policy {
    border-inline-start: 0;
    border-top: 1px solid var(--line);
    padding: 12px 0 0;
  }
}
</style>
