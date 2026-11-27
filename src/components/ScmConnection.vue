<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import SourceLoginHeader from './SourceLoginHeader.vue';
import { api, desktop } from '../api';
import type { ScmStatus } from '../types';
const props = withDefaults(
  defineProps<{ scope?: 'project' | 'logs'; sessionExpired?: boolean; disabled?: boolean }>(),
  {
    scope: 'project',
    sessionExpired: false,
    disabled: false,
  },
);
const title = computed(() => (props.scope === 'logs' ? '日志 SCM 登录' : '项目 SCM 登录'));
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
    const logs = props.scope === 'logs';
    const login = logs ? api.logScmLogin : api.scmLogin;
    const logout = logs ? api.logScmLogout : api.scmLogout;
    const load = logs ? api.logScmStatus : api.scmStatus;
    const value =
      action === 'login'
        ? await login(baseUrl.value.trim(), username.value.trim(), password.value, remember.value)
        : action === 'logout'
          ? await logout()
          : await load();
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
  <section class="source-login" :aria-label="title">
    <SourceLoginHeader
      :title="title"
      :environment="scope === 'logs' ? '生产环境' : '项目 SCM'"
      :connected="!!status?.connected && !expired"
      :state="
        expired
          ? '已过期'
          : status?.connected
            ? '已登录'
            : pending
              ? '检查中'
              : status
                ? '未登录'
                : '状态未知'
      "
      :disabled="!desktop || pending || disabled"
      @refresh="request('status')"
      @logout="request('logout')"
    />
    <p v-if="scope === 'logs'" class="hint">
      请填写生产 SCM 的 HTTPS 根地址。本通道只查生产日志，不连接测试跳板机，也不会使用项目 SCM
      的账号。
    </p>
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
          ><input v-model="remember" type="checkbox" />记住登录（本机私有存储）</label
        >
        <div class="connection-actions">
          <button class="button small primary" type="submit">
            {{ pending ? '处理中…' : '登录' }}
          </button>
        </div>
      </fieldset>
    </form>
    <p v-if="status?.message && !expired" class="hint" role="status">{{ status.message }}</p>
    <p v-if="error" role="alert">{{ error }}</p>
    <slot v-if="scope === 'logs'" name="options" :connected="!!status?.connected && !expired" />
  </section>
</template>
