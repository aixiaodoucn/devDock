<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { api, desktop } from '../api';
import type { ScmStatus } from '../types';
const props = withDefaults(defineProps<{ sessionExpired?: boolean; disabled?: boolean }>(), {
  sessionExpired: false,
  disabled: false,
});
const emit = defineEmits<{ connected: [status: ScmStatus]; change: [status: ScmStatus] }>();
const baseUrl = ref('');
const username = ref('');
const password = ref('');
const remember = ref(false);
const status = ref<ScmStatus | null>(null);
const expired = computed(
  () =>
    props.sessionExpired ||
    /sessionExpired|session.expired|会话.*过期/i.test(status.value?.message ?? ''),
);
const pending = ref(false);
const error = ref('');
let generation = 0;
let disposed = false;
function accept(value: ScmStatus) {
  status.value = value;
  baseUrl.value = value.baseUrl ?? baseUrl.value;
  username.value = value.username ?? username.value;
  remember.value = value.remembered;
  emit('change', value);
  if (value.connected) emit('connected', value);
}
async function request(action: 'status' | 'login' | 'logout') {
  if (!desktop || pending.value || props.disabled) return;
  if (action === 'login') {
    try {
      const url = new URL(baseUrl.value);
      if (!['http:', 'https:'].includes(url.protocol) || !username.value.trim() || !password.value)
        throw new Error();
    } catch {
      error.value = '请填写有效的 SCM URL、用户名和密码。';
      return;
    }
  }
  const token = ++generation;
  pending.value = true;
  error.value = '';
  try {
    const value =
      action === 'login'
        ? await api.scmLogin(
            baseUrl.value.trim(),
            username.value.trim(),
            password.value,
            remember.value,
          )
        : action === 'logout'
          ? await api.scmLogout()
          : await api.scmStatus();
    if (disposed || token !== generation) return;
    password.value = '';
    accept(value);
  } catch (reason) {
    if (!disposed && token === generation) {
      // 仅识别受控错误类别，不显示可能包含服务端回显的原始错误。
      password.value = '';
      const expiredSession = String(reason instanceof Error ? reason.message : reason).startsWith(
        'SessionExpired:',
      );
      if (expiredSession || action === 'login') {
        accept({
          connected: false,
          baseUrl: baseUrl.value,
          username: username.value,
          remembered: false,
          message: expiredSession ? 'SessionExpired: SCM 会话已过期，请重新登录。' : null,
        });
      }
      error.value = expiredSession
        ? ''
        : action === 'login'
          ? '登录失败，请检查地址与凭据后重试。'
          : 'SCM 状态更新失败，请重试。';
    }
  } finally {
    if (!disposed && token === generation) pending.value = false;
  }
}
watch(
  () => props.sessionExpired,
  (expired) => {
    if (expired) {
      ++generation;
      pending.value = false;
      password.value = '';
      status.value = null;
    }
  },
);
onMounted(() => {
  void request('status');
});
onBeforeUnmount(() => {
  disposed = true;
  ++generation;
  password.value = '';
});
</script>

<template>
  <section class="scm-connection" aria-label="SCM 连接">
    <div class="connection-heading">
      <strong>SCM 连接</strong
      ><span>{{ status?.connected && !expired ? '已连接' : '未连接' }}</span>
    </div>
    <p v-if="!desktop" class="hint">浏览器预览：SCM 登录需要桌面应用。</p>
    <p v-if="expired" role="alert">会话已过期，请重新登录。</p>
    <div v-if="status?.connected && !expired" class="connection-summary">
      <dl>
        <div>
          <dt>SCM URL</dt>
          <dd>{{ status.baseUrl || '未提供地址' }}</dd>
        </div>
        <div>
          <dt>登录用户</dt>
          <dd>{{ status.username || '未提供用户名' }}</dd>
        </div>
      </dl>
      <div class="connection-actions">
        <button
          class="button small"
          type="button"
          :disabled="!desktop || pending || disabled"
          @click="request('status')"
        >
          刷新状态
        </button>
        <button
          class="button small"
          type="button"
          :disabled="!desktop || pending || disabled"
          @click="request('logout')"
        >
          退出登录
        </button>
      </div>
    </div>
    <p v-else-if="pending && !status" class="hint" role="status">正在检查登录状态…</p>
    <form v-else @submit.prevent="request('login')">
      <fieldset :disabled="!desktop || pending || disabled">
        <label
          >SCM URL<input
            v-model="baseUrl"
            name="baseUrl"
            placeholder="https://example.invalid"
            autocomplete="url"
        /></label>
        <label>用户名<input v-model="username" name="username" autocomplete="username" /></label>
        <label
          >密码<input
            v-model="password"
            name="password"
            type="password"
            autocomplete="current-password"
        /></label>
        <label class="remember"
          ><input v-model="remember" type="checkbox" />记住登录（系统凭据存储）</label
        >
        <div class="connection-actions">
          <button class="button small primary" type="submit">
            {{ pending ? '处理中…' : '登录' }}</button
          ><button class="button small" type="button" @click="request('status')">刷新状态</button>
        </div>
      </fieldset>
    </form>
    <p v-if="status?.message && !expired" class="hint" role="status">{{ status.message }}</p>
    <p v-if="error" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.scm-connection {
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--paper);
}
.connection-heading,
.connection-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}
.connection-heading {
  justify-content: space-between;
  margin-bottom: 12px;
}
.connection-heading span,
.hint {
  color: var(--muted);
  font-size: 12px;
}
.connection-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px 20px;
}
.connection-summary dl {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 24px;
  min-width: 0;
  margin: 0;
  font-size: 12px;
}
.connection-summary dl > div {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}
.connection-summary dt {
  color: var(--muted);
  flex-shrink: 0;
}
.connection-summary dd {
  min-width: 0;
  margin: 0;
  color: var(--ink);
  overflow-wrap: anywhere;
}
fieldset {
  display: grid;
  grid-template-columns: minmax(180px, 2fr) 1fr 1fr;
  gap: 10px;
  margin: 0;
  padding: 0;
  border: 0;
}
label {
  display: grid;
  gap: 5px;
  color: var(--text-secondary);
  font-size: 12px;
}
input:not([type='checkbox']) {
  min-width: 0;
  width: 100%;
}
.remember {
  display: flex;
  flex-direction: row;
  align-items: center;
  grid-column: 1 / -1;
}
.connection-actions {
  grid-column: 1 / -1;
  flex-wrap: wrap;
}
[role='alert'] {
  color: var(--danger-text);
  font-size: 12px;
}
@media (max-width: 700px) {
  fieldset {
    grid-template-columns: 1fr;
  }
}
</style>
