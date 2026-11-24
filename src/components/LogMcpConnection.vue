<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { api, desktop, type LogMcpScanScope } from '../api';
import type { LogMcpStatus, McpClient, McpClientStatus } from '../types';

const CLIENT_NAMES = { claude: 'Claude Code', codex: 'Codex CLI' } as const;
const STATE_LABELS = {
  missing: '未检测到客户端',
  available: '可注册',
  configured: '已注册',
  conflict: '配置冲突',
  unsupported: '暂不支持',
} satisfies Record<McpClientStatus['state'], string>;
const clientStatus = ref<Partial<Record<McpClient, McpClientStatus>>>({});
const clientBusy = ref({ claude: false, codex: false });
const template = ref('');
const templateBusy = ref(false);
const templateMessage = ref('');

const status = ref<LogMcpStatus | null>(null);
const visibleToken = ref('');
const pending = ref(false);
const error = ref('');
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
  if (!desktop || !active || pending.value || !status.value?.enabled || rawEnabled.value === null)
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

async function request(action: 'status' | 'enable' | 'disable' | 'token') {
  if (!desktop || pending.value || (action === 'token' && !status.value?.enabled)) return;
  pending.value = true;
  error.value = '';
  visibleToken.value = '';
  try {
    // 凭据仅在主动显示时读取，隐藏或离开偏好设置即清除。
    if (action === 'token') {
      const token = await api.logMcpToken();
      if (active) visibleToken.value = token;
    } else {
      rawDialog.value?.close();
      rawEnabled.value = null;
      rawError.value = false;
      const load = {
        status: api.logMcpStatus,
        enable: api.logMcpEnable,
        disable: api.logMcpDisable,
      }[action];
      const result = await load();
      if (active) {
        status.value = result;
        if (result.enabled) await syncRaw();
      }
    }
  } catch {
    if (active) {
      if (action !== 'token') status.value = null;
      error.value =
        action === 'token' ? '读取令牌失败，请重试。' : '未能确认服务状态，请刷新状态。';
    }
  } finally {
    if (active) pending.value = false;
  }
}
async function updateClient(client: McpClient, action: 'status' | 'connect' | 'disconnect') {
  if (clientBusy.value[client]) return;
  clientBusy.value[client] = true;
  try {
    const call = {
      status: api.mcpClientStatus,
      connect: api.mcpClientConnect,
      disconnect: api.mcpClientDisconnect,
    }[action];
    clientStatus.value[client] = await call(client);
  } catch {
    // 操作失败后实际注册状态未知，由用户刷新后再决定下一步。
    delete clientStatus.value[client];
  } finally {
    clientBusy.value[client] = false;
  }
}
function refresh() {
  if (!desktop) return;
  void request('status');
  void syncScanScope();
  void updateClient('claude', 'status');
  void updateClient('codex', 'status');
}
async function showTemplate() {
  templateBusy.value = true;
  templateMessage.value = '';
  try {
    template.value = await api.mcpClientTemplate();
  } catch {
    templateMessage.value = '读取配置失败，请重试。';
  } finally {
    templateBusy.value = false;
  }
}
async function copyTemplate() {
  try {
    await navigator.clipboard.writeText(template.value);
    templateMessage.value = '已复制 JSON';
  } catch {
    templateMessage.value = '复制失败，请手动选中并复制下方 JSON。';
  }
}
onMounted(refresh);
onBeforeUnmount(() => {
  active = false;
  visibleToken.value = '';
});
</script>

<template>
  <section v-if="desktop" class="log-mcp" aria-label="日志 MCP">
    <div class="section-heading">
      <h3>
        日志 MCP 服务 · 生产环境
        <span class="service-badge" :class="{ online: status?.enabled }">{{
          status ? (status.enabled ? '已启用' : '已停用') : pending ? '正在检查…' : '状态未知'
        }}</span>
      </h3>
      <button
        v-if="status"
        class="button small"
        type="button"
        :disabled="pending"
        @click="request(status.enabled ? 'disable' : 'enable')"
      >
        {{ status.enabled ? '停用' : '启用' }}
      </button>
      <button
        class="button small"
        type="button"
        :disabled="pending || clientBusy.claude || clientBusy.codex"
        @click="refresh"
      >
        刷新状态
      </button>
    </div>
    <p class="hint">
      适用于所有已保存映射的后端仓库。启用时凭据存入系统凭据库；请确认已获授权向 AI 发送日志。
    </p>
    <div class="query-options">
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
            :disabled="pending || !status?.enabled || rawEnabled === null"
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
    <h4>客户端注册</h4>
    <p class="hint">注册后仍需启用上方日志服务，客户端才能访问日志。</p>
    <dl aria-live="polite">
      <template v-for="(name, client) in CLIENT_NAMES" :key="client">
        <dt>{{ name }}</dt>
        <dd>
          <span>{{
            clientBusy[client]
              ? '处理中…'
              : clientStatus[client]
                ? STATE_LABELS[clientStatus[client].state]
                : '状态未知，请刷新'
          }}</span>
          <small v-if="!clientBusy[client]">{{ clientStatus[client]?.message }}</small>
          <button
            v-if="clientStatus[client]?.state === 'available'"
            class="button small"
            type="button"
            :disabled="clientBusy[client]"
            @click="updateClient(client, 'connect')"
          >
            注册 {{ name }}
          </button>
          <button
            v-if="clientStatus[client]?.state === 'configured'"
            class="button small"
            type="button"
            :disabled="clientBusy[client]"
            @click="updateClient(client, 'disconnect')"
          >
            断开 {{ name }}
          </button>
        </dd>
      </template>
    </dl>
    <details>
      <summary>CodeBuddy / WorkBuddy 通用 stdio 配置</summary>
      <p class="hint">将下方 JSON 合并到客户端的 MCP 配置中。此方案尚未经这两个客户端实机验证。</p>
      <button
        v-if="!template"
        class="button small"
        type="button"
        :disabled="templateBusy"
        @click="showTemplate"
      >
        查看 JSON 配置
      </button>
      <button v-else class="button small" type="button" @click="copyTemplate">复制 JSON</button>
      <span v-if="templateMessage" role="status">{{ templateMessage }}</span>
      <pre v-if="template">{{ template }}</pre>
    </details>
    <details @toggle="!($event.currentTarget as HTMLDetailsElement).open && (visibleToken = '')">
      <summary>HTTP 手动连接</summary>
      <dl v-if="status?.enabled">
        <dt>服务地址</dt>
        <dd>
          <code>{{ status.endpoint }}</code>
        </dd>
        <dt>访问令牌</dt>
        <dd>
          <code v-if="visibleToken">{{ visibleToken }}</code>
          <button v-if="visibleToken" class="button small" type="button" @click="visibleToken = ''">
            隐藏令牌
          </button>
          <button
            v-else
            class="button small"
            type="button"
            :disabled="pending"
            @click="request('token')"
          >
            显示令牌
          </button>
        </dd>
      </dl>
      <p v-if="!status?.enabled" class="hint">启用日志服务后可查看地址和访问令牌。</p>
      <p class="hint">
        在 MCP 客户端的用户级本地配置中选择 HTTP 传输，填入本次服务地址，并将 Authorization
        请求头设为
        <code>Bearer &lt;本次令牌&gt;</code
        >。每次启动服务都会更换端口和令牌，请同步更新本地配置。公共仓库中只使用占位符，禁止提交真实账号、域名、令牌或其他私有信息。
      </p>
    </details>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
  </section>
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
.log-mcp {
  min-width: 0;
  container-type: inline-size;
  font-size: 14px;
  line-height: 1.5;
  color: var(--ink);
  padding: clamp(16px, 2vw, 26px);
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--paper);
}
.log-mcp .section-heading {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.log-mcp .section-heading {
  justify-content: flex-start;
}
.log-mcp h3 {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  margin: 0 auto 0 0;
  font-size: 17px;
}
.log-mcp h4 {
  font-size: 14px;
  margin: 20px 0 0;
}
.service-badge {
  font-size: 12px;
  font-weight: 500;
  color: var(--muted);
  background: var(--surface-subtle);
  border-radius: 20px;
  padding: 2px 9px;
}
.service-badge.online {
  color: var(--success-text);
  background: var(--success-soft);
}
.log-mcp .hint,
.log-mcp small {
  font-size: 12px;
  color: var(--muted);
  margin: 6px 0;
  line-height: 1.55;
}
.query-options {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  margin: 18px 0;
  padding: 16px;
  background: var(--surface-subtle);
  border-block: 1px solid var(--line);
}
.query-options fieldset {
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
.log-mcp details {
  border-top: 1px solid var(--line);
  padding-block: 9px;
  font-size: 13px;
}
.log-mcp dl {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 8px 16px;
  align-items: baseline;
}
.log-mcp dd {
  margin: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  overflow-wrap: anywhere;
}
.log-mcp dd .button {
  margin-inline-start: auto;
}
.log-mcp dt {
  font-weight: 600;
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
  .log-mcp small {
    flex-basis: 100%;
  }
}
.log-mcp pre {
  max-width: 100%;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
.log-mcp summary {
  cursor: pointer;
}
</style>
