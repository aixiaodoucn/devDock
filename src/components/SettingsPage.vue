<script setup lang="ts">
import { ref, watch } from 'vue';
import {
  Download,
  Upload,
  RefreshCw,
  SlidersHorizontal,
  Terminal,
  ShieldCheck,
} from 'lucide-vue-next';
import type { Settings, Tools } from '../types';
import { desktop } from '../api';
import PathInput from './PathInput.vue';
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
  detect: [];
  export: [];
  import: [];
  error: [message: string];
}>();
const draft = ref({ ...props.settings });
watch(
  () => props.settings,
  (value) => {
    draft.value = { ...value };
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
    <div class="settings-columns">
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
        <form class="form-stack" @submit.prevent="emit('save', { ...draft })">
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
          <div class="form-end">
            <button class="button primary" :disabled="saving || busy">
              {{ saving ? '保存中…' : '保存设置' }}
            </button>
          </div>
        </form>
      </section>
      <div class="form-stack">
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
    </div>
  </section>
</template>
