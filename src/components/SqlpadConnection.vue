<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import SourceLoginHeader from './SourceLoginHeader.vue';
import { api, desktop } from '../api';
import type { SqlpadStatus } from '../types';

const status = ref<SqlpadStatus | null>(null);
const baseUrl = ref('');
const email = ref('');
const password = ref('');
const candidate = ref('');
const confirmed = ref(false);
const busy = ref(false);
const message = ref('');
const selected = computed(() =>
  status.value?.connections.find((item) => item.id === status.value?.selectedId),
);
const choice = computed(() =>
  status.value?.connections.find((item) => item.id === candidate.value),
);

async function update(load: () => Promise<SqlpadStatus>, error: string) {
  if (!desktop || busy.value) return;
  busy.value = true;
  message.value = '';
  confirmed.value = false;
  try {
    const result = await load();
    status.value = result;
    baseUrl.value = result.baseUrl || baseUrl.value;
    candidate.value = result.selectedId ?? '';
  } catch {
    // IPC 错误可能包含请求参数，只展示固定提示，并要求重新获取可信状态。
    status.value = null;
    candidate.value = '';
    message.value = error;
  } finally {
    busy.value = false;
  }
}
function refresh() {
  return update(api.sqlpadStatus, '读取 SQLPad 状态失败，请重试。');
}
function login() {
  if (busy.value || !email.value.trim() || !password.value) return;
  let origin: string;
  try {
    const address = baseUrl.value.trim();
    if (!/^https:\/\/[^/?#@\\\s]+\/?$/i.test(address)) throw new Error();
    origin = new URL(address).origin;
  } catch {
    message.value = '请输入 HTTPS 根地址，仅包含主机和可选端口。';
    return;
  }
  // 命令发出后立即清空输入，即使请求失败也不保留登录凭据。
  void update(
    () => api.sqlpadLogin(origin, email.value.trim(), password.value),
    '登录失败，请刷新状态后重新登录。',
  );
  email.value = '';
  password.value = '';
}
function selectConnection() {
  if (
    !status.value?.connected ||
    !choice.value ||
    !confirmed.value ||
    candidate.value === selected.value?.id
  )
    return;
  void update(
    () => api.sqlpadSelectConnection(candidate.value),
    '未能确认所选连接，会话可能已失效。请刷新状态。',
  );
}
onMounted(refresh);
</script>

<template>
  <section v-if="desktop" class="source-login" aria-label="生产 SQLPad 数据库">
    <SourceLoginHeader
      title="SQLPad 登录"
      :connected="!!status?.connected"
      :state="status?.connected ? '已登录' : busy ? '检查中' : status ? '未登录' : '状态未知'"
      :disabled="busy"
      refresh-label="刷新 SQLPad 状态"
      @refresh="refresh"
    />
    <p class="hint">
      仅支持 MySQL / PostgreSQL
      只读连接。表结构与数据查询均须在主窗口逐次确认，结果将返回给发起请求的 AI 客户端。
    </p>
    <p v-if="message" role="alert" class="error">{{ message }}</p>
    <p v-if="!status" role="status">{{ busy ? '正在确认 SQLPad 状态…' : 'SQLPad 状态未知' }}</p>
    <template v-else-if="status.connected">
      <div class="connection-summary">
        <dl>
          <div>
            <dt>SQLPad URL</dt>
            <dd>{{ status.baseUrl }}</dd>
          </div>
          <div>
            <dt>当前连接</dt>
            <dd>{{ selected?.name ?? '未选择' }}</dd>
          </div>
        </dl>
      </div>
      <label v-if="status.connections.length"
        >选择唯一允许的连接
        <select v-model="candidate" :disabled="busy" @change="confirmed = false">
          <option disabled value="">请选择连接</option>
          <option v-for="item in status.connections" :key="item.id" :value="item.id">
            {{ item.name }}（{{ item.driver }}）
          </option>
        </select>
      </label>
      <p v-else>当前账号没有可选连接。</p>
      <label v-if="choice && candidate !== selected?.id" class="confirm"
        ><input v-model="confirmed" type="checkbox" :disabled="busy" />我已确认「{{
          choice.name
        }}」在数据库端仅有只读权限。</label
      >
      <div class="actions">
        <button
          v-if="status.connections.length"
          class="button primary"
          type="button"
          :disabled="busy || !confirmed || !choice || candidate === selected?.id"
          @click="selectConnection"
        >
          保存只读连接
        </button>
        <button
          class="button"
          type="button"
          :disabled="busy"
          @click="update(api.sqlpadLogout, '退出状态未知，请刷新状态。')"
        >
          退出 SQLPad
        </button>
      </div>
    </template>
    <form v-else autocomplete="off" @submit.prevent="login">
      <fieldset :disabled="busy">
        <label
          >SQLPad HTTPS 根地址<input
            v-model="baseUrl"
            required
            type="url"
            placeholder="https://sqlpad.example.com"
            :disabled="busy"
        /></label>
        <label
          >邮箱<input v-model="email" type="email" autocomplete="off" required :disabled="busy"
        /></label>
        <label
          >密码<input
            v-model="password"
            type="password"
            autocomplete="off"
            required
            :disabled="busy"
        /></label>
        <p role="status" class="hint auth-note">尚未登录或会话已失效。邮箱和密码仅用于本次登录。</p>
        <div class="actions">
          <button type="submit" class="button small primary" :disabled="busy">登录 SQLPad</button>
        </div>
      </fieldset>
    </form>
  </section>
</template>
