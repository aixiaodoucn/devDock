<script setup lang="ts">
import './source-login.css';
defineProps<{
  title: string;
  state: string;
  environment?: string;
  connected: boolean;
  disabled: boolean;
  refreshLabel?: string;
}>();
defineEmits<{ refresh: []; logout: [] }>();
</script>
<template>
  <header class="source-heading">
    <div class="source-title">
      <small>{{ environment ?? '生产环境' }}</small>
      <h3>{{ title }}</h3>
    </div>
    <div class="session-actions">
      <span class="source-state" role="status" :class="{ online: connected }">{{ state }}</span>
      <button
        type="button"
        class="button small"
        :aria-label="refreshLabel || '刷新状态'"
        :disabled="disabled"
        @click="$emit('refresh')"
      >
        刷新状态
      </button>
      <button
        v-if="connected"
        type="button"
        class="button small negative"
        :disabled="disabled"
        @click="$emit('logout')"
      >
        退出登录
      </button>
    </div>
  </header>
</template>
<style scoped>
.source-heading,
.source-title {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.session-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  margin-inline-start: auto;
}
.source-title {
  margin-right: auto;
}
.source-title small {
  font-size: 11px;
  color: var(--accent-text);
  background: var(--accent-soft);
  border-radius: 4px;
  padding: 2px 6px;
}
.source-title h3 {
  margin: 0;
  color: var(--ink);
  font-size: 15px;
  line-height: 1.4;
}
.source-state {
  color: var(--muted);
  font-size: 12px;
}
.source-state.online {
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--success-soft);
  color: var(--success-text);
}
</style>
