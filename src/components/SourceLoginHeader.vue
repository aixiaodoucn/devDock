<script setup lang="ts">
import './source-login.css';
defineProps<{
  title: string;
  state: string;
  connected: boolean;
  disabled: boolean;
  refreshLabel?: string;
}>();
defineEmits<{ refresh: [] }>();
</script>
<template>
  <header class="source-heading">
    <div class="source-title">
      <small>生产环境</small>
      <h3>{{ title }}</h3>
    </div>
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
  color: var(--success-text);
}
</style>
