<script setup lang="ts">
import { FolderOpen } from 'lucide-vue-next';
import { api } from '../api';
const props = withDefaults(
  defineProps<{ modelValue: string; placeholder?: string; directory?: boolean; label?: string }>(),
  { directory: true },
);
const emit = defineEmits<{ 'update:modelValue': [value: string]; error: [message: string] }>();
async function browse() {
  try {
    const path = await api.choosePath(props.directory ?? true);
    if (path) emit('update:modelValue', path);
  } catch (error) {
    emit('error', String(error));
  }
}
</script>
<template>
  <div class="path-input">
    <input
      :aria-label="label || placeholder || '路径'"
      :value="modelValue"
      :placeholder="placeholder"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <button
      type="button"
      class="icon-button"
      :aria-label="`选择${label || '路径'}`"
      title="选择路径"
      @click="browse"
    >
      <FolderOpen :size="17" />
    </button>
  </div>
</template>
