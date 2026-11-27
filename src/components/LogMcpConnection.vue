<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { api, desktop } from '../api';
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
let active = true;

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
      const load = {
        status: api.logMcpStatus,
        enable: api.logMcpEnable,
        disable: api.logMcpDisable,
      }[action];
      const result = await load();
      if (active) status.value = result;
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
  <section v-if="desktop" class="log-mcp" aria-label="本机 MCP 服务">
    <div class="section-heading">
      <h3>
        本机 MCP 服务
        <span class="service-badge" :class="{ online: status?.enabled }">{{
          status ? (status.enabled ? '已启用' : '已停用') : pending ? '正在检查…' : '状态未知'
        }}</span>
      </h3>
      <button
        v-if="status"
        class="button small"
        :class="status.enabled ? 'negative' : 'positive'"
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
      启用后会记住状态，下次打开 Dev Dock 时如登录会话仍有效会自动恢复；手动停用后不再自动启动。 SCM
      日志适用于所有已保存映射的后端仓库。项目工具使用「项目 SCM
      登录」的账号；创建、修改项目、解除模块关联或执行 Git 操作前，需在主窗口逐次批准。SQLPad
      使用独立连接和审批，不受 SCM 日志原文开关控制。启用时，Dev Dock 将桥接凭据保存在本机，供 stdio
      客户端读取。
    </p>
    <h4>客户端注册</h4>
    <p class="hint">注册后仍需启用本机 MCP 服务，客户端才能调用工具。</p>
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
            class="button small positive"
            type="button"
            :disabled="clientBusy[client]"
            @click="updateClient(client, 'connect')"
          >
            注册 {{ name }}
          </button>
          <button
            v-if="clientStatus[client]?.state === 'configured'"
            class="button small negative"
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
      <p v-if="!status?.enabled" class="hint">启用本机 MCP 服务后可查看地址和访问令牌。</p>
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
