<script setup lang="ts">
import { ref, watch } from 'vue';
import {
  Download,
  Upload,
  RefreshCw,
  SlidersHorizontal,
  Terminal,
  ShieldCheck,
  Palette,
  Check,
} from 'lucide-vue-next';
import type { Settings, Tools, ThemeName } from '../types';
import { THEME_OPTIONS } from '../types';
import { desktop } from '../api';
import PathInput from './PathInput.vue';
import AiToolsEditor from './AiToolsEditor.vue';
const props = defineProps<{
  settings: Settings;
  tools: Tools;
  detecting: boolean;
  saving: boolean;
  busy: boolean;
  projectCount: number;
}>();
const emit = defineEmits<{
  save: [settings: Settings];
  theme: [theme: ThemeName];
  detect: [];
  export: [settings: Settings];
  import: [];
  error: [message: string];
}>();
// 设置草稿需深拷贝，编辑终端命令时不能提前修改已保存的配置。
const cloneSettings = (settings: Settings): Settings => JSON.parse(JSON.stringify(settings));
const draft = ref(cloneSettings(props.settings));
const settingsForm = ref<HTMLFormElement>();
function validateDraft() {
  for (const [index, tool] of draft.value.aiTools.entries()) {
    const field = !tool.name.trim() ? '名称' : !tool.command.trim() ? '执行命令' : '';
    if (!field) continue;
    emit(
      'error',
      field === '名称'
        ? `请填写第 ${index + 1} 个终端命令的名称。`
        : `请为「${tool.name.trim()}」填写执行命令，例如 claude、codex 或 git pull。`,
    );
    settingsForm.value
      ?.querySelector<HTMLInputElement>(`[aria-label="终端命令 ${index + 1} ${field}"]`)
      ?.focus();
    return false;
  }
  return true;
}
function saveDraft() {
  if (!props.saving && !props.busy && validateDraft()) emit('save', cloneSettings(draft.value));
}
function exportDraft() {
  // 导出当前表单快照，包含尚未保存的终端命令，不改动已保存设置。
  if (!props.saving && !props.busy && validateDraft()) emit('export', cloneSettings(draft.value));
}
watch(
  () => props.settings,
  (value, previous) => {
    // 快速切换主题时保留尚未保存的工具输入，只同步外观与构建偏好。
    if (
      value.hbuilderxPath !== previous.hbuilderxPath ||
      value.terminal !== previous.terminal ||
      value.jdkPath !== previous.jdkPath ||
      value.defaultAiToolId !== previous.defaultAiToolId ||
      JSON.stringify(value.aiTools) !== JSON.stringify(previous.aiTools)
    ) {
      draft.value = cloneSettings(value);
    } else {
      draft.value.theme = value.theme;
      draft.value.openOutputAfterBuild = value.openOutputAfterBuild;
    }
  },
  { deep: true },
);
</script>
<template>
  <section class="settings-page">
    <div class="page-heading">
      <div>
        <div class="eyebrow">PREFERENCES</div>
        <h1>让工具适应你的习惯<span class="heading-dot">.</span></h1>
        <p>配置本机开发环境，备份你的工作台。</p>
      </div>
      <SlidersHorizontal :size="32" class="heading-icon" />
    </div>
    <form ref="settingsForm" class="settings-config" novalidate @submit.prevent="saveDraft">
      <div class="settings-tools-grid">
        <section class="panel">
          <div class="panel-heading">
            <div>
              <h2>开发工具</h2>
              <p>项目可以覆盖这里的默认配置。</p>
            </div>
            <button
              type="button"
              class="button small"
              :disabled="detecting || !desktop"
              @click="emit('detect')"
            >
              <RefreshCw :size="14" :class="{ spinning: detecting }" />{{
                detecting ? '检测中' : '自动检测'
              }}
            </button>
          </div>
          <div class="form-stack">
            <label
              >HBuilderX 应用路径<PathInput
                v-model="draft.hbuilderxPath"
                placeholder="/Applications/HBuilderX.app"
                @error="emit('error', $event)"
            /></label>
            <label
              >默认终端<select v-model="draft.terminal">
                <option v-for="tool in tools.terminals" :key="tool.path" :value="tool.path">
                  {{ tool.name }}
                </option>
                <option
                  v-if="!tools.terminals.some((t) => t.path === draft.terminal)"
                  :value="draft.terminal"
                >
                  {{ draft.terminal || '请选择终端' }}
                </option>
              </select></label
            >
            <PathInput
              v-model="draft.terminal"
              placeholder="手动指定终端应用路径"
              label="默认终端路径"
              @error="emit('error', $event)"
            />
            <div>
              <label for="default-jdk" class="field-label">默认 JDK 版本</label>
              <div class="jdk-select-row">
                <select id="default-jdk" v-model="draft.jdkPath">
                  <option value="">使用系统 JAVA_HOME</option>
                  <option v-for="tool in tools.jdks" :key="tool.path" :value="tool.path">
                    {{ tool.name }}
                  </option>
                  <option
                    v-if="draft.jdkPath && !tools.jdks.some((t) => t.path === draft.jdkPath)"
                    :value="draft.jdkPath"
                  >
                    {{ draft.jdkPath }}
                  </option></select
                ><button
                  type="button"
                  class="button"
                  :disabled="detecting || !desktop"
                  @click="emit('detect')"
                >
                  <RefreshCw :size="14" :class="{ spinning: detecting }" />{{
                    detecting ? '获取中…' : '获取本地版本'
                  }}
                </button>
              </div>
              <p v-if="tools.jdks.length" class="field-help">
                已发现 {{ tools.jdks.length }} 个本地 JDK，请在下拉框选择。
              </p>
            </div>
            <PathInput
              v-model="draft.jdkPath"
              placeholder="手动指定 JDK Home"
              label="默认 JDK 路径"
              @error="emit('error', $event)"
            />
          </div>
        </section>
        <section class="panel settings-ai-panel">
          <AiToolsEditor
            v-model="draft.aiTools"
            v-model:default-tool-id="draft.defaultAiToolId"
            :disabled="saving || busy"
          />
        </section>
      </div>
      <div class="form-end settings-save">
        <button class="button primary" :disabled="saving || busy">
          {{ saving ? '保存中…' : '保存设置' }}
        </button>
      </div>
    </form>
    <div class="settings-support-grid">
      <section class="panel backup-panel">
        <div class="backup-header">
          <div class="backup-symbol"><ShieldCheck :size="22" /></div>
          <h2>配置随你迁移</h2>
          <p>
            导出项目、平台、操作命令、工具设置及终端命令配置。重新安装或更换设备后，从 JSON
            备份恢复。
          </p>
        </div>
        <div class="backup-count">
          <span>{{ projectCount }}</span> 个项目配置
        </div>
        <div class="button-row">
          <button class="button" :disabled="busy || saving" @click="exportDraft">
            <Download :size="16" />导出 JSON</button
          ><button class="button" :disabled="busy || saving" @click="emit('import')">
            <Upload :size="16" />导入 JSON
          </button>
        </div>
        <p class="field-help">
          导出包含当前表单中的
          终端命令名称、执行内容和启用状态。导入前会校验格式，并提供合并或替换选项。备份不包含源码、构建产物或
          JDK；命令中的敏感值也会随配置导出，请妥善保存。
        </p>
      </section>
      <section class="environment-note">
        <Terminal :size="20" />
        <div>
          <h3>macOS 本地执行</h3>
          <p>
            HBuilderX 菜单操作需要辅助功能和自动化权限。终端任务在所选终端中查看日志和停止；App
            构建日志在工作台显示。
          </p>
        </div>
      </section>
    </div>
    <section class="appearance-section" aria-labelledby="appearance-title">
      <div class="section-heading">
        <div>
          <div class="eyebrow">MAKE IT YOURS</div>
          <h2 id="appearance-title"><Palette :size="18" />外观主题</h2>
          <p>为你的工作空间，选一种舒服的颜色。切换即保存。</p>
        </div>
        <span class="appearance-note">六种色彩，同样专注</span>
      </div>
      <div class="theme-options">
        <button
          v-for="theme in THEME_OPTIONS"
          :key="theme.id"
          type="button"
          :class="['theme-option', { selected: settings.theme === theme.id }]"
          :aria-pressed="settings.theme === theme.id"
          :title="theme.description"
          :disabled="saving"
          @click="emit('theme', theme.id)"
        >
          <div :class="['theme-preview', theme.id]" aria-hidden="true">
            <div class="theme-preview-sidebar"><span></span><i></i><i></i><i></i></div>
            <div class="theme-preview-main">
              <span class="theme-preview-line"></span>
              <div class="theme-preview-cards"><i></i><i></i><i></i></div>
              <span class="theme-preview-line short"></span
              ><span class="theme-preview-action"></span>
            </div>
            <span class="theme-preview-check" v-if="settings.theme === theme.id"
              ><Check :size="16"
            /></span>
          </div>
          <div class="theme-option-caption">
            <span
              ><strong>{{ theme.name }}</strong
              ><small>{{ theme.english }}</small></span
            ><span class="theme-color-swatch" :style="{ background: theme.color }"></span>
          </div>
        </button>
      </div>
    </section>
  </section>
</template>

<style scoped>
.settings-config {
  margin-bottom: 36px;
}
.settings-tools-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  align-items: start;
  gap: 16px;
}
.settings-tools-grid > .panel {
  min-width: 0;
}
.settings-ai-panel :deep(.ai-tools-editor) {
  border-top: 0;
  padding-top: 0;
}
.settings-save {
  margin-top: 14px;
}
.settings-support-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 16px;
  align-items: start;
  margin-bottom: 24px;
}
.backup-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.backup-header .backup-symbol {
  flex-shrink: 0;
  margin-bottom: 0;
}
.backup-header h2 {
  flex-shrink: 0;
  margin: 0;
  font-size: 16px;
}
.backup-header p {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--muted);
}
.appearance-section {
  border-top: 1px solid var(--line);
  padding-top: 22px;
}
@media (max-width: 1100px) {
  .settings-tools-grid,
  .settings-support-grid {
    grid-template-columns: 1fr;
  }
}
</style>
