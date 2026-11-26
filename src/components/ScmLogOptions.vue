<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue';
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

const matchLimit = ref<number | ''>('');
const savedMatchLimit = ref<number | null>(null);
const limitBusy = ref(false);
const limitError = ref('');
const canSaveLimit = computed(
  () =>
    !limitBusy.value &&
    savedMatchLimit.value !== null &&
    validLimit(matchLimit.value) &&
    matchLimit.value !== savedMatchLimit.value,
);

function validLimit(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= 10000;
}

async function syncMatchLimit(limit?: number) {
  if (limitBusy.value || (limit !== undefined && !canSaveLimit.value)) return;
  limitBusy.value = true;
  limitError.value = '';
  try {
    const result =
      limit === undefined ? await api.logMcpMatchLimit() : await api.logMcpSetMatchLimit(limit);
    if (!validLimit(result)) throw new Error();
    // 后端确认的值同时用于回显和判断是否有待保存的更改。
    savedMatchLimit.value = result;
    matchLimit.value = result;
  } catch {
    limitError.value =
      limit === undefined ? '读取匹配行数上限失败，请刷新状态。' : '保存匹配行数上限失败，请重试。';
  } finally {
    limitBusy.value = false;
  }
}

onMounted(() => {
  void syncMatchLimit();
  void syncScanScope();
  if (props.serviceEnabled) void syncRaw();
});
onBeforeUnmount(() => {
  active = false;
});
</script>
<template>
  <div class="query-options source-options" role="group" aria-label="SCM 日志查询选项">
    <div class="source-option">
      <span class="source-option-label" id="log-scope-label">查询实例</span>
      <fieldset
        class="scope-picker"
        aria-labelledby="log-scope-label"
        :disabled="scopeBusy || scanScope === null"
      >
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
      </fieldset>
      <p class="hint">未指定实例时使用此范围。实例范围跨重启保留。</p>
      <p v-if="scopeError" class="error" role="alert">{{ scopeError }}</p>
    </div>
    <div class="source-option">
      <label class="source-option-label" for="log-raw-checkbox">允许返回日志原文</label>
      <label class="confirm">
        <input
          id="log-raw-checkbox"
          type="checkbox"
          aria-label="允许返回日志原文"
          :checked="rawEnabled === true"
          aria-describedby="raw-policy-hint"
          :disabled="pending || !serviceEnabled || rawEnabled === null"
          @click.prevent="rawEnabled ? setRaw(false) : rawDialog?.showModal()"
        />
        <span id="raw-policy-hint"
          >原文授权仅对确认时的日志 SCM
          站点和账号有效。停用或退出会暂停服务，服务未启动时不会返回原文。在同一站点用相同账号重新启用服务可恢复授权。取消勾选会撤销已保存的授权。</span
        >
      </label>
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
    <div class="source-option">
      <label class="source-option-label" for="log-mcp-match-limit">匹配行数上限</label>
      <div class="limit-controls">
        <input
          id="log-mcp-match-limit"
          v-model.number="matchLimit"
          type="number"
          min="1"
          max="10000"
          step="1"
          aria-describedby="match-limit-hint"
          :disabled="limitBusy || savedMatchLimit === null"
        />
        <button
          type="button"
          class="button"
          :disabled="!canSaveLimit"
          @click="validLimit(matchLimit) && syncMatchLimit(matchLimit)"
        >
          保存上限
        </button>
      </div>
      <p class="hint" id="match-limit-hint">
        显式指定的 maxLines 受此上限约束，默认上限为 500 行，可设为 1–10000 的整数。省略 maxLines
        时，搜索最多返回 20 行或所设上限中的较小值，扫描默认仅统计匹配行数。
      </p>
      <p v-if="limitError" class="error" role="alert">{{ limitError }}</p>
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
.limit-controls {
  display: flex;
  align-items: center;
  gap: 8px;
}
.limit-controls input {
  max-width: 100px;
  min-width: 0;
}
.limit-controls button {
  flex: none;
}
.query-options .scope-picker {
  display: inline-flex;
  justify-self: start;
  min-width: 0;
  margin: 0;
  padding: 0;
  gap: 0;
  border: 1px solid var(--line);
  border-radius: 7px;
  overflow: hidden;
  background: var(--paper);
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
@container (max-width: 400px) {
  .scope-picker span {
    padding-inline: 6px;
  }
}
</style>
