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
  export: [];
  import: [];
  error: [message: string];
}>();
// 设置草稿需深拷贝，编辑 AI 工具时不能提前修改已保存的配置。
const cloneSettings = (settings: Settings): Settings => JSON.parse(JSON.stringify(settings));
const draft = ref(cloneSettings(props.settings));
watch(
  () => props.settings,
  (value, previous) => {
    // 快速切换主题时保留尚未保存的工具输入，只同步外观与构建偏好。
    if (
      value.hbuilderxPath !== previous.hbuilderxPath ||
      value.terminal !== previous.terminal ||
      value.jdkPath !== previous.jdkPath ||
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
    <form class="settings-config" @submit.prevent="emit('save', cloneSettings(draft))">
      <div class="settings-tools-grid">
        <section class="panel">
          <div class="panel-heading">
            <div>
              <h2>开发工具</h2>
              <p>项目可以覆盖这里的默认配置。</p>
            </div>
            <button class="button small" :disabled="detecting || !desktop" @click="emit('detect')">
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
          <AiToolsEditor v-model="draft.aiTools" :disabled="saving || busy" />
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
        <div class="backup-symbol"><ShieldCheck :size="26" /></div>
        <h2>配置随你迁移</h2>
        <p>导出项目、平台、操作命令和工具设置。重新安装或更换设备后，从 JSON 备份恢复。</p>
        <div class="backup-count">
          <span>{{ projectCount }}</span> 个项目配置
        </div>
        <div class="button-row">
          <button class="button" @click="emit('export')"><Download :size="16" />导出 JSON</button
          ><button class="button" :disabled="busy || saving" @click="emit('import')">
            <Upload :size="16" />导入 JSON
          </button>
        </div>
        <p class="field-help">
          导入前会校验格式，并提供合并或替换选项。备份不包含源码、构建产物或
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
  margin-bottom: 20px;
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
  grid-template-columns: minmax(0, 1fr) minmax(0, 0.75fr);
  gap: 16px;
  align-items: start;
  margin-bottom: 24px;
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
