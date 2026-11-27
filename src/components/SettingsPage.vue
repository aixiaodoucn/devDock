<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  Download,
  Upload,
  RefreshCw,
  SlidersHorizontal,
  Plug,
  Terminal,
  ShieldCheck,
  Palette,
  Check,
} from 'lucide-vue-next';
import type { Settings, Tools, ThemeName } from '../types';
import { NODE_MANAGER_LABELS, THEME_OPTIONS } from '../types';
import { api, desktop } from '../api';
import ScmLogOptions from './ScmLogOptions.vue';
import ScmConnection from './ScmConnection.vue';
import LogMcpConnection from './LogMcpConnection.vue';
import SqlpadConnection from './SqlpadConnection.vue';
import SqlpadExecutionHistory from './SqlpadExecutionHistory.vue';
import McpToolCatalog from './McpToolCatalog.vue';
import PathInput from './PathInput.vue';
import AiToolsEditor from './AiToolsEditor.vue';
import { isWindows } from '../platform';
import { terminalFeedback } from '../terminal';
const props = defineProps<{
  settings: Settings;
  tools: Tools;
  detecting: boolean;
  saving: boolean;
  busy: boolean;
  projectCount: number;
  focusProjectScm?: boolean;
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
const terminalNotice = computed(() => terminalFeedback(draft.value.terminal));
// 日志登录变化后重建卡片，立即丢弃旧会话令牌和在途请求。
const logSessionRevision = ref(0);
const MCP_TABS = [
  { id: 'connections', label: '连接与权限' },
  { id: 'catalog', label: '工具目录' },
  { id: 'history', label: '执行记录' },
] as const;
const mcpTab = ref<(typeof MCP_TABS)[number]['id']>(MCP_TABS[0].id);
function navigateMcpTabs(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const current = MCP_TABS.findIndex((tab) => tab.id === mcpTab.value);
  const adjacent =
    (current + (event.key === 'ArrowLeft' ? -1 : 1) + MCP_TABS.length) % MCP_TABS.length;
  mcpTab.value =
    MCP_TABS[event.key === 'Home' ? 0 : event.key === 'End' ? MCP_TABS.length - 1 : adjacent].id;
  (event.currentTarget as HTMLElement)
    .querySelector<HTMLButtonElement>(`#mcp-${mcpTab.value}-tab`)
    ?.focus();
}
const SETTINGS_SECTIONS = [
  { id: 'tools', label: '开发工具', icon: SlidersHorizontal },
  { id: 'commands', label: '终端命令', icon: Terminal },
  { id: 'backup', label: '配置随你迁移', icon: ShieldCheck },
  { id: 'mcp', label: 'MCP 服务', icon: Plug },
  { id: 'appearance', label: '外观主题', icon: Palette },
] as const;
const activeSection = ref<(typeof SETTINGS_SECTIONS)[number]['id']>(
  props.focusProjectScm ? 'mcp' : 'tools',
);
const projectScm = ref<InstanceType<typeof ScmConnection>>();
onMounted(() => {
  // 聚焦卡片会同时将其滚动到可视区域。
  if (props.focusProjectScm) projectScm.value?.$el.focus();
});
const settingsForm = ref<HTMLFormElement>();
// 保存和导出共用草稿；错误字段可能在另一个模块，先切换再聚焦。
function focusField(section: 'tools' | 'commands', selector: string) {
  activeSection.value = section;
  void nextTick(() => settingsForm.value?.querySelector<HTMLElement>(selector)?.focus());
}
const nodeMode = computed({
  get: () => draft.value.nodeManager || '',
  set: (value: NonNullable<Settings['nodeManager']>) => {
    draft.value.nodeManager = value;
  },
});
const nodeRoot = computed({
  get: () => (nodeMode.value === 'fnm' ? draft.value.fnmDir : draft.value.nvmDir) || '',
  set: (value: string) => {
    if (nodeMode.value === 'fnm') draft.value.fnmDir = value;
    else if (nodeMode.value === 'nvm') draft.value.nvmDir = value;
  },
});
const detectingNode = ref(false);
const nodeRoots = ref<string[]>([]);
const nodeDetectionMessage = ref('');
let nodeDetectionSequence = 0;
const isAbsoluteRoot = (path: string) =>
  /^(?:\/|[a-z]:[\\/]|\\\\[^\\]+\\[^\\]+)/i.test(path.trim());
watch(nodeMode, () => {
  // 切换模式立即使旧检测失效，避免异步响应写入另一管理器的目录。
  nodeDetectionSequence++;
  detectingNode.value = false;
  nodeRoots.value = [];
  nodeDetectionMessage.value = '';
});
onBeforeUnmount(() => nodeDetectionSequence++);
function detectTools() {
  if (props.detecting || props.saving || props.busy || detectingNode.value || !desktop) return;
  emit('detect');
  void detectNodeRoot(true);
}
async function detectNodeRoot(scanAll = false) {
  const manager = nodeMode.value;
  if (props.saving || props.busy || detectingNode.value || !desktop) return;
  const selectedManager = manager === 'nvm' || manager === 'fnm' ? manager : undefined;
  if (!selectedManager && !scanAll) return;
  const sequence = ++nodeDetectionSequence;
  const initialRoot = nodeRoot.value;
  detectingNode.value = true;
  nodeRoots.value = [];
  nodeDetectionMessage.value = '';
  const isCurrent = () => sequence === nodeDetectionSequence && manager === nodeMode.value;
  try {
    const versions = await api.nodeVersions(
      selectedManager ? { manager: selectedManager } : undefined,
    );
    if (!isCurrent() || nodeRoot.value !== initialRoot || props.saving || props.busy) return;
    // 未选管理器时只展示发现结果，避免检测改变已有项目的环境含义。
    if (!selectedManager) {
      const managers = [...new Set(versions.map((version) => version.manager || 'nvm'))];
      nodeDetectionMessage.value = managers.length
        ? `已检测到 ${managers.join('、')}，共 ${versions.length} 个 Node.js 版本。请选择公共环境模式后检测目录，并保存设置。`
        : '未检测到 nvm/fnm 已安装版本。可使用终端默认环境；此检测不运行 Node.js，未核验终端当前版本。';
      return;
    }
    const roots = [
      ...new Set(
        versions
          .filter((version) => (version.manager || 'nvm') === manager)
          .map((version) => (manager === 'fnm' ? version.fnmDir : version.nvmDir)?.trim() || '')
          .filter(isAbsoluteRoot),
      ),
    ];
    nodeRoots.value = roots;
    if (roots.length === 1) {
      nodeRoot.value = roots[0];
      nodeDetectionMessage.value = '已检测到唯一版本数据目录，请保存设置使其生效。';
    } else {
      nodeDetectionMessage.value = roots.length
        ? '检测到多个版本数据目录，请选择一个目录，不会自动覆盖当前配置。'
        : '未检测到本地版本数据目录，可手动选择目录，或使用终端默认模式。';
    }
  } catch (error) {
    if (isCurrent())
      nodeDetectionMessage.value = `检测失败：${String(error)}。可手动选择目录，或使用终端默认模式。`;
  } finally {
    if (isCurrent()) detectingNode.value = false;
  }
}
function validateDraft() {
  const manager = nodeMode.value;
  if (manager && !['system', 'nvm', 'fnm'].includes(manager)) {
    emit('error', '请选择有效的公共 Node.js 环境模式。');
    focusField('tools', '#settings-node-manager');
    return false;
  }
  if ((manager === 'nvm' || manager === 'fnm') && !isAbsoluteRoot(nodeRoot.value)) {
    emit('error', `请为 ${NODE_MANAGER_LABELS[manager]} 填写非空的绝对版本数据目录。`);
    focusField('tools', '[aria-label="公共 Node.js 版本数据目录"]');
    return false;
  }
  for (const [index, tool] of draft.value.aiTools.entries()) {
    const field = !tool.name.trim() ? '名称' : !tool.command.trim() ? '执行命令' : '';
    if (!field) continue;
    emit(
      'error',
      field === '名称'
        ? `请填写第 ${index + 1} 个终端命令的名称。`
        : `请为「${tool.name.trim()}」填写执行命令，例如 claude、codex 或 git pull。`,
    );
    focusField('commands', `[aria-label="终端命令 ${index + 1} ${field}"]`);
    return false;
  }
  return true;
}
function saveDraft() {
  if (!props.saving && !props.busy && !props.detecting && !detectingNode.value && validateDraft())
    emit('save', cloneSettings(draft.value));
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
      value.nodeManager !== previous.nodeManager ||
      value.nvmDir !== previous.nvmDir ||
      value.fnmDir !== previous.fnmDir ||
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
        <div class="eyebrow">偏好设置</div>
        <h1>让工具适应你的习惯<span class="heading-dot">.</span></h1>
        <p>配置本机开发环境，备份你的工作台。</p>
      </div>
      <SlidersHorizontal :size="32" class="heading-icon" />
    </div>
    <div class="preferences-layout">
      <nav class="preferences-nav" aria-label="偏好设置分类">
        <button
          v-for="item in SETTINGS_SECTIONS"
          :key="item.id"
          type="button"
          :aria-current="activeSection === item.id ? 'page' : undefined"
          @click="activeSection = item.id"
        >
          <component :is="item.icon" :size="16" aria-hidden="true" />{{ item.label }}
        </button>
      </nav>
      <div class="preferences-content">
        <form
          v-show="activeSection === 'tools' || activeSection === 'commands'"
          ref="settingsForm"
          class="settings-config"
          novalidate
          @submit.prevent="saveDraft"
        >
          <div class="settings-tools-grid">
            <section v-show="activeSection === 'tools'" class="panel">
              <div class="panel-heading">
                <div>
                  <h2>开发工具</h2>
                  <p>项目可以覆盖这里的默认配置。</p>
                </div>
                <div class="button-row settings-tool-actions">
                  <button
                    type="button"
                    class="button small"
                    :disabled="detecting || detectingNode || saving || busy || !desktop"
                    @click="detectTools"
                  >
                    <RefreshCw :size="14" :class="{ spinning: detecting || detectingNode }" />{{
                      detecting || detectingNode ? '检测中' : '自动检测'
                    }}
                  </button>
                  <button
                    type="submit"
                    class="button primary small"
                    :disabled="saving || busy || detecting || detectingNode"
                  >
                    {{ saving ? '保存中…' : '保存设置' }}
                  </button>
                </div>
              </div>
              <div class="form-stack">
                <label
                  >HBuilderX 应用路径<PathInput
                    v-model="draft.hbuilderxPath"
                    :placeholder="
                      isWindows
                        ? 'C:\\Program Files\\HBuilderX\\HBuilderX.exe'
                        : '/Applications/HBuilderX.app'
                    "
                    :directory="!isWindows"
                    @error="emit('error', $event)"
                /></label>
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
                <fieldset class="settings-node-fields form-stack" :disabled="saving || busy">
                  <div class="settings-node-heading">
                    <label for="settings-node-manager" class="field-label">公共 Node.js 环境</label>
                    <button
                      v-if="nodeMode === 'nvm' || nodeMode === 'fnm'"
                      type="button"
                      class="button small"
                      :disabled="saving || busy || detectingNode || !desktop"
                      @click="detectNodeRoot()"
                    >
                      <RefreshCw :size="14" :class="{ spinning: detectingNode }" />{{
                        detectingNode ? '检测中…' : '检测本地目录'
                      }}
                    </button>
                  </div>
                  <div class="settings-node-selection">
                    <select id="settings-node-manager" v-model="nodeMode">
                      <option v-if="!nodeMode" value="">未设置（保留已有项目配置）</option>
                      <option
                        v-for="(label, manager) in NODE_MANAGER_LABELS"
                        :key="manager"
                        :value="manager"
                      >
                        {{ label }}
                      </option>
                    </select>
                    <PathInput
                      v-if="nodeMode === 'nvm' || nodeMode === 'fnm'"
                      :key="nodeMode"
                      v-model="nodeRoot"
                      label="公共 Node.js 版本数据目录"
                      :placeholder="
                        nodeMode === 'nvm'
                          ? 'nvm 版本数据根目录（NVM_DIR / NVM_HOME）'
                          : 'fnm 版本数据根目录（FNM_DIR）'
                      "
                      @error="emit('error', $event)"
                    />
                  </div>
                  <p class="field-help">
                    未设置时，保留各项目已有的 Node.js
                    配置；未指定版本的项目使用终端默认环境。设置后，各项目统一使用公共 Node.js
                    环境。
                  </p>
                  <template v-if="nodeMode === 'nvm' || nodeMode === 'fnm'">
                    <p class="field-help">
                      {{
                        nodeMode === 'nvm' ? 'NVM_DIR / NVM_HOME' : 'FNM_DIR'
                      }}：请选择非空的绝对目录，仅保存配置，不执行或安装管理器。
                    </p>
                    <label v-if="nodeRoots.length > 1" for="settings-node-root"
                      >选择检测到的版本数据目录
                      <select
                        id="settings-node-root"
                        :value="nodeRoots.includes(nodeRoot) ? nodeRoot : ''"
                        @change="nodeRoot = ($event.target as HTMLSelectElement).value"
                      >
                        <option value="" disabled>请选择目录</option>
                        <option v-for="root in nodeRoots" :key="root" :value="root">
                          {{ root }}
                        </option>
                      </select>
                    </label>
                  </template>
                  <p v-if="nodeDetectionMessage" class="field-help" role="status">
                    {{ nodeDetectionMessage }}
                  </p>
                </fieldset>
              </div>
            </section>
            <section v-show="activeSection === 'commands'" class="panel settings-ai-panel">
              <AiToolsEditor
                v-model="draft.aiTools"
                v-model:default-tool-id="draft.defaultAiToolId"
                :disabled="saving || busy"
              >
                <template #actions>
                  <button
                    type="submit"
                    class="button primary small"
                    :disabled="saving || busy || detecting || detectingNode"
                  >
                    {{ saving ? '保存中…' : '保存设置' }}
                  </button>
                </template>
                <template #terminal>
                  <div class="settings-terminal-fields">
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
                    <label
                      >默认终端路径
                      <PathInput
                        v-model="draft.terminal"
                        :placeholder="
                          isWindows ? '选择已适配终端的 EXE 文件' : '手动指定终端应用路径'
                        "
                        :directory="!isWindows"
                        label="默认终端路径"
                        @error="emit('error', $event)"
                      />
                    </label>
                  </div>
                  <p v-if="terminalNotice.message" class="note" role="status">
                    {{ terminalNotice.message }}
                  </p>
                </template>
              </AiToolsEditor>
            </section>
          </div>
        </form>
        <div
          class="settings-support-grid"
          v-show="activeSection === 'tools' || activeSection === 'backup'"
        >
          <section v-show="activeSection === 'backup'" class="panel backup-panel">
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
          <section v-show="activeSection === 'tools'" class="environment-note">
            <Terminal :size="20" />
            <div>
              <h3>{{ isWindows ? 'Windows 本地执行' : 'macOS 本地执行' }}</h3>
              <p v-if="isWindows">
                终端命令在所选终端的 PowerShell 会话执行，日志和停止在终端中处理。Tabby 需确认
                Run，Warp 需注册启动协议。HBuilderX 菜单自动化及 APK/WGT
                构建暂不支持，不会修改系统执行策略。
              </p>
              <p v-else>
                HBuilderX 菜单操作需要辅助功能和自动化权限。终端任务在所选终端中查看日志和停止；App
                构建日志在工作台显示。
              </p>
            </div>
          </section>
        </div>
        <div v-if="activeSection === 'mcp'" class="settings-log-tools mcp-settings">
          <template v-if="desktop">
            <div
              role="tablist"
              aria-label="MCP 服务视图"
              class="mcp-tabs"
              @keydown="navigateMcpTabs"
            >
              <button
                v-for="tab in MCP_TABS"
                :key="tab.id"
                type="button"
                role="tab"
                :id="`mcp-${tab.id}-tab`"
                :aria-controls="`mcp-${tab.id}-panel`"
                :aria-selected="mcpTab === tab.id"
                :tabindex="mcpTab === tab.id ? 0 : -1"
                @click="mcpTab = tab.id"
              >
                {{ tab.label }}
              </button>
            </div>
            <section
              role="tabpanel"
              id="mcp-history-panel"
              aria-labelledby="mcp-history-tab"
              :hidden="mcpTab !== 'history'"
            >
              <SqlpadExecutionHistory v-if="mcpTab === 'history'" />
            </section>
            <section
              role="tabpanel"
              id="mcp-catalog-panel"
              aria-labelledby="mcp-catalog-tab"
              :hidden="mcpTab !== 'catalog'"
            >
              <McpToolCatalog v-if="mcpTab === 'catalog'" />
            </section>
            <section
              role="tabpanel"
              id="mcp-connections-panel"
              aria-labelledby="mcp-connections-tab"
              :hidden="mcpTab !== 'connections'"
            >
              <div v-if="mcpTab === 'connections'" class="settings-log-tools">
                <ScmConnection ref="projectScm" scope="project" tabindex="-1" />
                <div class="connection-accounts" role="group" aria-label="生产数据源登录">
                  <ScmConnection scope="logs" @change="logSessionRevision++">
                    <template #options="{ connected }">
                      <ScmLogOptions :key="logSessionRevision" :connected="connected" />
                    </template>
                  </ScmConnection>
                  <SqlpadConnection />
                </div>
                <LogMcpConnection :key="logSessionRevision" />
              </div>
            </section>
          </template>
          <section v-else class="panel">
            <h2>MCP 服务</h2>
            <p>请在 Dev Dock 桌面主窗口配置项目 SCM、生产日志 SCM、SQLPad 连接和 MCP 客户端。</p>
          </section>
        </div>
        <section
          v-if="activeSection === 'appearance'"
          class="appearance-section"
          aria-labelledby="appearance-title"
        >
          <div class="section-heading">
            <div>
              <div class="eyebrow">外观</div>
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
      </div>
    </div>
  </section>
</template>

<style scoped>
.preferences-layout {
  display: grid;
  grid-template-columns: clamp(118px, 13vw, 166px) minmax(0, 1fr);
  gap: clamp(12px, 2vw, 28px);
  align-items: start;
}
.preferences-nav {
  position: sticky;
  top: 12px;
  display: grid;
  gap: 4px;
  padding: 4px 10px 4px 0;
  border-right: 1px solid var(--line);
}
.preferences-nav button {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 38px;
  border: 0;
  border-radius: 7px;
  padding: 7px 9px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.preferences-nav button:hover {
  background: var(--surface-subtle);
}
.preferences-nav button[aria-current] {
  background: var(--accent-soft);
  color: var(--accent-text);
  font-weight: 650;
  box-shadow: inset 2px 0 var(--accent);
}
.preferences-content {
  min-width: 0;
}
/* 颜色按操作含义区分；仅影响 MCP 页，不改变共享登录组件在其他页面的外观。 */
.mcp-settings :deep(.button:not(.primary)) {
  --action-text: var(--accent-text);
  --action-fill: var(--accent-soft);
  --action-border: var(--accent-border);
  color: var(--action-text);
  background: var(--action-fill);
  border-color: var(--action-border);
}
.mcp-settings :deep(.button.positive) {
  --action-text: var(--success-text);
  --action-fill: var(--success-soft);
  --action-border: color-mix(in srgb, var(--success-text) 32%, var(--paper));
}
.mcp-settings :deep(.button.negative) {
  --action-text: var(--danger);
  --action-fill: var(--danger-soft);
  --action-border: var(--danger-border);
}
.mcp-settings :deep(.button:not(.primary):hover:not(:disabled)) {
  background: color-mix(in srgb, var(--action-text) 9%, var(--action-fill));
  border-color: var(--action-text);
}
.mcp-settings :deep(.button:disabled) {
  color: var(--muted);
  background: var(--surface-subtle);
  border-color: var(--line);
  box-shadow: none;
}
.mcp-tabs {
  display: flex;
  gap: 18px;
  border-bottom: 1px solid var(--line);
}
.mcp-tabs button {
  border: 0;
  border-bottom: 2px solid transparent;
  background: transparent;
  padding: 8px 2px;
  font: inherit;
  font-size: 13px;
  color: var(--muted);
  cursor: pointer;
}
.mcp-tabs button[aria-selected='true'] {
  border-color: var(--accent);
  color: var(--accent-text);
  font-weight: 650;
}
.mcp-tabs button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.connection-accounts {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 16px;
}
.settings-log-tools {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  align-items: start;
  gap: 16px;
  margin: 0;
}
.settings-config {
  margin-bottom: 36px;
}
.settings-tools-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
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
.settings-terminal-fields {
  display: grid;
  grid-template-columns: var(--terminal-columns);
  align-items: start;
  column-gap: var(--terminal-column-gap);
  row-gap: 14px;
  padding-bottom: 16px;
  margin-bottom: 16px;
  border-bottom: 1px solid var(--line);
}
.settings-terminal-fields > label:first-child {
  grid-column: 2;
}
.settings-terminal-fields > label:last-child {
  grid-column: 3 / -1;
}
.settings-node-fields {
  min-width: 0;
  margin: 0;
  padding: 16px 0 0;
  border: 0;
  border-top: 1px solid var(--line);
}
.settings-node-heading {
  display: flex;
  align-items: center;
  gap: 12px;
}
.settings-node-heading .field-label {
  margin: 0;
}
.settings-node-selection {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.settings-node-selection > select {
  width: 240px;
  max-width: 100%;
}
.settings-node-selection > :deep(.path-input) {
  flex: 1;
  min-width: 240px;
}
.settings-tool-actions {
  flex-shrink: 0;
  flex-wrap: wrap;
  justify-content: flex-end;
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
  padding-top: 0;
  border-top: 0;
}
@media (max-width: 1100px) {
  .settings-tools-grid,
  .settings-support-grid {
    grid-template-columns: 1fr;
  }
  .settings-terminal-fields > label:last-child {
    grid-column: 2 / -1;
    grid-row: 2;
  }
}
</style>
