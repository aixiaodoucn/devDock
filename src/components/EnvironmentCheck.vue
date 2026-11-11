<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { CheckCircle2, CircleMinus, CircleX, ShieldCheck, TriangleAlert } from 'lucide-vue-next';
import { api, desktop } from '../api';
import type { EnvironmentReport, Project, Settings } from '../types';

const props = defineProps<{ project: Project; settings: Settings }>();
const emit = defineEmits<{ close: []; settings: []; edit: [] }>();
const report = ref<EnvironmentReport | null>(null);
const checking = ref(false);
const authorizing = ref(false);
const error = ref('');
const permissionMessage = ref('');
const dialog = ref<HTMLElement>();
const labels = { pass: '通过', warning: '警告', error: '错误', skipped: '已跳过' };
const icons = { pass: CheckCircle2, warning: TriangleAlert, error: CircleX, skipped: CircleMinus };
const counts = computed(() => ({
  pass: report.value?.items.filter((item) => item.status === 'pass').length ?? 0,
  error: report.value?.items.filter((item) => item.status === 'error').length ?? 0,
  warning: report.value?.items.filter((item) => item.status === 'warning').length ?? 0,
}));
let generation = 0;
let closed = false;
let previousFocus: HTMLElement | null = null;
function errorText(value: unknown) {
  return value instanceof Error ? value.message : String(value);
}
async function check() {
  if (!desktop || closed || checking.value || authorizing.value) return;
  const request = ++generation;
  checking.value = true;
  error.value = '';
  permissionMessage.value = '';
  try {
    const result = await api.checkEnvironment(props.project, props.settings);
    if (!closed && request === generation) report.value = result;
  } catch (value) {
    if (!closed && request === generation)
      error.value = `体检失败：${errorText(value)}。请点击重新体检重试。`;
  } finally {
    if (!closed && request === generation) checking.value = false;
  }
}
async function requestPermission() {
  if (!desktop || closed || checking.value || authorizing.value) return;
  const request = generation;
  authorizing.value = true;
  permissionMessage.value = '';
  try {
    // 请求接口的返回值不能代替权限体检；只由下一次只读检查确认实际状态。
    await api.requestAccessibilityPermission();
    if (!closed && request === generation)
      permissionMessage.value = '授权请求已发送，请在系统设置中确认后点击「重新体检」检查权限。';
  } catch (value) {
    if (!closed && request === generation)
      permissionMessage.value = `权限请求失败：${errorText(value)}。请重试或检查系统设置。`;
  } finally {
    if (!closed && request === generation) authorizing.value = false;
  }
}
function close() {
  closed = true;
  generation++;
  emit('close');
}
function keydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    close();
  }
  if (event.key !== 'Tab') return;
  const controls = dialog.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
  const first = controls?.[0],
    last = controls?.[controls.length - 1];
  if (
    event.shiftKey &&
    (document.activeElement === first || document.activeElement === dialog.value)
  ) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first?.focus();
  }
}
watch(
  () => [props.project, props.settings],
  () => {
    // 深度观察配置，同时使旧检查和授权结果失效。
    generation++;
    report.value = null;
    checking.value = false;
    authorizing.value = false;
    error.value = '';
    permissionMessage.value = '';
    void check();
  },
  { immediate: true, deep: true },
);
onMounted(async () => {
  previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  await nextTick();
  dialog.value?.focus();
});
onUnmounted(() => {
  closed = true;
  generation++;
  previousFocus?.focus();
});
</script>

<template>
  <div class="modal-backdrop" @click.self="close">
    <section
      ref="dialog"
      class="modal environment-check"
      role="dialog"
      aria-modal="true"
      aria-labelledby="environment-title"
      tabindex="-1"
      @keydown="keydown"
    >
      <header class="modal-header">
        <h2 id="environment-title"><ShieldCheck :size="20" />环境体检</h2>
        <button class="button" @click="close">关闭</button>
      </header>
      <div class="modal-body environment-body" :aria-busy="checking">
        <p class="environment-project">{{ project.name }} · 只读检查，不执行构建</p>
        <p v-if="!desktop" role="status">
          请在桌面应用中运行环境体检，浏览器预览无法访问本机环境和权限。
        </p>
        <p v-else-if="checking" role="status">正在检查项目环境…</p>
        <p v-if="error" class="environment-error" role="alert">{{ error }}</p>
        <template v-if="report">
          <p class="environment-summary">
            通过 {{ counts.pass }} · 错误 {{ counts.error }} · 警告 {{ counts.warning }}
          </p>
          <p class="environment-time">
            检查时间：<time :datetime="new Date(report.checkedAt).toISOString()">{{
              new Date(report.checkedAt).toLocaleString()
            }}</time>
          </p>
          <ul class="environment-items">
            <li v-for="item in report.items" :key="item.id" :class="item.status">
              <component :is="icons[item.status]" :size="18" aria-hidden="true" />
              <div class="environment-item">
                <div>
                  <strong>{{ item.label }}</strong
                  ><span class="environment-status">{{ labels[item.status] }}</span>
                </div>
                <p>{{ item.detail }}</p>
                <button
                  v-if="
                    item.action === 'request-accessibility' &&
                    item.status !== 'pass' &&
                    item.status !== 'skipped'
                  "
                  class="button small"
                  :disabled="checking || authorizing"
                  @click="requestPermission"
                >
                  {{ authorizing ? '授权处理中…' : '请求辅助功能权限' }}
                </button>
                <button
                  v-if="item.action === 'settings'"
                  class="button small"
                  @click="emit('settings')"
                >
                  偏好设置
                </button>
              </div>
            </li>
          </ul>
        </template>
        <p v-if="permissionMessage" role="status">{{ permissionMessage }}</p>
      </div>
      <footer class="modal-footer environment-footer">
        <button class="button" @click="emit('edit')">编辑项目</button
        ><button class="button" @click="emit('settings')">偏好设置</button
        ><button
          class="button primary"
          :disabled="!desktop || checking || authorizing"
          @click="check"
        >
          {{ checking ? '检查中…' : '重新体检' }}
        </button>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.environment-check {
  width: 620px;
  outline: none;
}
.environment-body {
  display: grid;
  gap: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}
.environment-project,
.environment-time {
  color: var(--muted);
}
.environment-summary {
  color: var(--ink);
  font-weight: 600;
}
.environment-time {
  font-size: 11px;
}
.environment-items {
  margin: 0;
  padding: 0;
  list-style: none;
}
.environment-items li {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px 0;
  border-top: 1px solid var(--line);
}
.environment-items svg {
  margin-top: 2px;
  flex-shrink: 0;
  color: var(--muted);
}
.environment-items .pass svg {
  color: var(--success-text);
}
.environment-items .warning svg {
  color: var(--warning-text);
}
.environment-items .error svg,
.environment-error {
  color: var(--danger-text);
}
.environment-item {
  flex: 1;
  min-width: 0;
}
.environment-item strong {
  color: var(--ink);
}
.environment-status {
  margin-left: 10px;
  color: var(--muted);
  font-size: 11px;
}
.environment-item p {
  margin: 5px 0;
  overflow-wrap: anywhere;
  line-height: 1.6;
}
.environment-footer {
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
}
</style>
