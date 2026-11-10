<script setup lang="ts">
import { Plus, Trash2, Sparkles } from 'lucide-vue-next';
import type { AiTool } from '../types';

const tools = defineModel<AiTool[]>({ required: true });
defineProps<{ disabled: boolean }>();
const presets = [
  { name: 'Claude Code', command: 'claude' },
  { name: 'Codex', command: 'codex' },
  { name: 'CodeBuddy', command: 'codebuddy' },
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
        <h2 id="ai-tools-title"><Sparkles :size="16" />AI 工具</h2>
        <p>配置已安装的命令行工具，保存后可在每个项目中打开。</p>
      </div>
    </div>
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
        <Plus :size="13" />自定义工具
      </button>
    </div>
    <p v-if="!tools.length" class="field-help">
      尚未配置 AI 工具。上方快捷添加仅填入命令示例，不会安装工具。
    </p>
    <fieldset
      v-for="(tool, index) in tools"
      :key="tool.id"
      class="ai-tool-row"
      :disabled="disabled"
    >
      <label class="ai-tool-enabled"
        ><input v-model="tool.enabled" type="checkbox" :aria-label="`启用 AI 工具 ${index + 1}`"
      /></label>
      <input
        v-model="tool.name"
        required
        :aria-label="`AI 工具 ${index + 1} 名称`"
        placeholder="工具名称"
      />
      <input
        v-model="tool.command"
        required
        :aria-label="`AI 工具 ${index + 1} 启动命令`"
        class="ai-tool-command"
        placeholder="启动命令，例如 claude"
      />
      <button
        type="button"
        class="icon-button danger-text"
        :aria-label="`移除 AI 工具 ${index + 1}`"
        @click="remove(tool.id)"
      >
        <Trash2 :size="14" />
      </button>
    </fieldset>
    <p class="field-help">
      仅勾选的工具会出现在项目入口。命令可包含参数或可执行文件路径，路径有空格时请加引号。请先在终端确认命令可用；安装和登录在对应工具中完成。AI
      工具使用终端默认环境，不切换项目的 Node 版本。
    </p>
  </section>
</template>

<style scoped>
.ai-tools-editor {
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
  grid-template-columns: 20px minmax(90px, 0.8fr) minmax(120px, 1.2fr) 28px;
  align-items: center;
  gap: 7px;
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
.ai-tool-command {
  font-family: var(--mono);
}
.ai-tool-enabled {
  display: flex;
  flex-direction: row;
  align-items: center;
}
@media (max-width: 1100px) {
  .ai-tool-row {
    grid-template-columns: 20px minmax(0, 1fr) 28px;
  }
  .ai-tool-command {
    grid-column: 2;
    grid-row: 2;
  }
  .ai-tool-row button {
    grid-column: 3;
    grid-row: 1;
  }
}
</style>
