<script setup lang="ts">
import { watch } from 'vue';
import { Plus, Trash2, Sparkles } from 'lucide-vue-next';
import type { AiTool } from '../types';

const tools = defineModel<AiTool[]>({ required: true });
const defaultToolId = defineModel<string>('defaultToolId', { required: true });
watch(
  () => tools.value.filter((tool) => tool.enabled).map((tool) => tool.id),
  (ids) => {
    if (defaultToolId.value && !ids.includes(defaultToolId.value)) defaultToolId.value = '';
  },
);
defineProps<{ disabled: boolean }>();
const presets = [
  { name: 'Claude Code', command: 'claude' },
  { name: 'Codex', command: 'codex' },
  { name: 'CodeBuddy', command: 'codebuddy' },
  { name: 'Git Pull', command: 'git pull' },
];
function add(name = '', command = '') {
  tools.value = [...tools.value, { id: crypto.randomUUID(), name, command, enabled: true }];
}
function remove(id: string) {
  tools.value = tools.value.filter((tool) => tool.id !== id);
}
</script>

<template>
  <section class="ai-tools-editor" aria-labelledby="ai-tools-title">
    <div class="panel-heading">
      <div>
        <h2 id="ai-tools-title"><Sparkles :size="16" />终端命令</h2>
        <p>配置常用命令，保存后可在任意项目目录中执行。</p>
      </div>
      <slot name="actions" />
    </div>
    <slot name="terminal" />
    <p class="field-help">
      点击命令右侧「设为默认」，未单独选择命令的项目会共用它；再次点击默认按钮可取消。
    </p>
    <div class="ai-presets">
      <button
        v-for="preset in presets"
        :key="preset.name"
        type="button"
        class="button small"
        :disabled="disabled || tools.some((tool) => tool.name === preset.name)"
        @click="add(preset.name, preset.command)"
      >
        <Plus :size="12" />{{ preset.name }}
      </button>
      <button type="button" class="text-button" :disabled="disabled" @click="add()">
        <Plus :size="13" />自定义命令
      </button>
    </div>
    <p v-if="!tools.length" class="field-help">
      尚未配置终端命令。上方快捷添加仅填入命令示例，不会安装工具。
    </p>
    <fieldset
      v-for="(tool, index) in tools"
      :key="tool.id"
      class="ai-tool-row"
      :disabled="disabled"
    >
      <label class="ai-tool-enabled"
        ><input v-model="tool.enabled" type="checkbox" :aria-label="`启用终端命令 ${index + 1}`"
      /></label>
      <input
        v-model="tool.name"
        required
        :aria-label="`终端命令 ${index + 1} 名称`"
        placeholder="命令名称"
      />
      <input
        v-model="tool.command"
        required
        :aria-label="`终端命令 ${index + 1} 执行命令`"
        class="ai-tool-command"
        placeholder="执行命令，例如 claude 或 git pull"
      />
      <button
        type="button"
        :class="['button small ai-default-button', { 'is-default': defaultToolId === tool.id }]"
        :disabled="disabled || !tool.enabled"
        :aria-pressed="defaultToolId === tool.id"
        :aria-label="`${defaultToolId === tool.id ? '取消默认' : '设为默认'}终端命令 ${index + 1}`"
        :title="defaultToolId === tool.id ? '点击取消默认命令' : '未单独配置的项目将使用此命令'"
        @click="defaultToolId = defaultToolId === tool.id ? '' : tool.id"
      >
        {{ defaultToolId === tool.id ? '默认命令' : '设为默认' }}
      </button>
      <button
        type="button"
        class="icon-button danger-text"
        :aria-label="`移除终端命令 ${index + 1}`"
        @click="remove(tool.id)"
      >
        <Trash2 :size="14" />
      </button>
    </fieldset>
    <p class="field-help">
      仅勾选的命令会出现在项目入口。命令可包含参数或可执行文件路径，路径有空格时请加引号。请先在终端确认命令可用；安装和登录在对应工具中完成。命令使用终端默认环境，不切换项目的
      Node 版本。
    </p>
  </section>
</template>

<style scoped>
.ai-tools-editor {
  /* 顶部终端选择与命令行共享列宽，保持名称列对齐。 */
  --terminal-columns: 20px minmax(90px, 0.8fr) minmax(120px, 1.2fr) 92px 28px;
  --terminal-column-gap: 7px;
  border-top: 1px solid var(--line);
  padding-top: 18px;
}
.ai-tools-editor h2 {
  display: flex;
  align-items: center;
  gap: 7px;
}
.ai-presets {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 7px;
  margin-bottom: 12px;
}
.ai-tool-row {
  display: grid;
  grid-template-columns: var(--terminal-columns);
  align-items: center;
  gap: var(--terminal-column-gap);
  border: 0;
  padding: 0;
  margin: 0 0 9px;
  min-width: 0;
}
.ai-tool-row input {
  font-size: 11px;
  padding: 8px;
}
.ai-tool-row input[type='checkbox'] {
  padding: 0;
}
.ai-default-button.is-default {
  color: var(--accent-text);
  background: var(--accent-soft);
  border-color: var(--accent-border);
}
.ai-tool-command {
  font-family: var(--mono);
}
.ai-tool-enabled {
  display: flex;
  flex-direction: row;
  align-items: center;
}
@media (max-width: 1100px) {
  .ai-tools-editor {
    --terminal-columns: 20px minmax(0, 1fr) 92px 28px;
  }
  .ai-tool-command {
    grid-column: 2 / 4;
    grid-row: 2;
  }
  .ai-default-button {
    grid-column: 3;
    grid-row: 1;
  }
  .ai-tool-row .icon-button {
    grid-column: 4;
    grid-row: 1;
  }
}
</style>
