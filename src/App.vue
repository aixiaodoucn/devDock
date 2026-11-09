<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  Plus,
  Settings2,
  Search,
  ArrowUpRight,
  FolderOpen,
  MoreHorizontal,
  Play,
  Package,
  Smartphone,
  Layers,
  Terminal,
  RefreshCw,
  Check,
  X,
  AlertCircle,
  Square,
  FileBox,
  ChevronRight,
  Download,
  Pencil,
  Trash2,
  Cpu,
  Copy,
  Palette,
} from 'lucide-vue-next';
import ProjectDialog from './components/ProjectDialog.vue';
import GitBranch from './components/GitBranch.vue';
import SettingsPage from './components/SettingsPage.vue';
import { api, desktop } from './api';
import { applyTheme } from './theme';
import { stopMenuAction } from './actions';
import {
  defaultConfig,
  newProject,
  mergeConfig,
  PLATFORM_LABELS,
  TYPE_LABELS,
  THEME_OPTIONS,
} from './types';
import type {
  AppConfig,
  Project,
  Settings,
  ThemeName,
  Tools,
  Task,
  TaskEvent,
  BuildMode,
  ManifestInfo,
  TaskStatus,
  Action,
} from './types';
const config = ref<AppConfig>(defaultConfig());
const selectedId = ref('');
const page = ref<'project' | 'settings'>('project');
const search = ref('');
const selectedPlatform = ref('app');
const editing = ref<Project | null>(null);
const deleting = ref<Project | null>(null);
const pendingImport = ref<AppConfig | null>(null);
const importMode = ref<'merge' | 'replace'>('merge');
const saving = ref(false);
const detecting = ref(false);
const loading = ref(true);
const loadFailed = ref(false);
const submitting = ref(false);
const refreshing = ref(false);
const tools = ref<Tools>({ hbuilderxPath: '', terminals: [], jdks: [] });
const notice = ref<{ message: string; error: boolean } | null>(null);
const tasks = ref<Task[]>([]);
const logElement = ref<HTMLElement>();
const selectedTaskId = ref('');
const manifest = ref<ManifestInfo | null>(null);
const versionName = ref('');
const versionCode = ref('');
const buildMode = ref<BuildMode>('apk');
const showProjectMenu = ref(false);
let unlisten: UnlistenFn | undefined;
let projectSelectionUnlisten: UnlistenFn | undefined;
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
let manifestRequest = 0;
const project = computed(() => config.value.projects.find((item) => item.id === selectedId.value));
const filteredProjects = computed(() =>
  config.value.projects.filter((item) =>
    item.name.toLowerCase().includes(search.value.toLowerCase()),
  ),
);
const projectTasks = computed(() =>
  tasks.value.filter((item) => item.projectId === selectedId.value),
);
const currentTask = computed(
  () =>
    projectTasks.value.find((item) => item.id === selectedTaskId.value) || projectTasks.value[0],
);
const busy = computed(
  () => submitting.value || tasks.value.some((task) => task.status === 'running'),
);
const actions = computed(
  () =>
    project.value?.commands.filter((action) => action.platform === selectedPlatform.value) || [],
);
const currentAppId = computed(() =>
  buildMode.value === 'wgt-ios' ? project.value?.iosAppId : project.value?.androidAppId,
);
const modeLabels: Record<BuildMode, string> = {
  apk: 'Android APK',
  'wgt-android': 'Android WGT',
  'wgt-ios': 'iOS WGT',
};
const statusLabels: Record<TaskStatus, string> = {
  running: '执行中',
  success: '已完成',
  error: '执行失败',
  cancelled: '已取消',
  triggered: '已触发',
};
function notify(message: string, error = false) {
  notice.value = { message, error };
  if (noticeTimer) clearTimeout(noticeTimer);
  if (!error)
    noticeTimer = setTimeout(() => {
      notice.value = null;
    }, 6000);
}
function fail(reason: unknown) {
  notify(reason instanceof Error ? reason.message : String(reason), true);
}
async function load() {
  loading.value = true;
  loadFailed.value = false;
  try {
    config.value = await api.load();
    selectedId.value = config.value.projects[0]?.id || '';
  } catch (error) {
    loadFailed.value = true;
    fail(error);
  } finally {
    loading.value = false;
  }
}
async function detect(showResult = false) {
  detecting.value = true;
  try {
    tools.value = await api.detect();
    if (showResult)
      notify(
        tools.value.jdks.length
          ? `已获取 ${tools.value.jdks.length} 个本地 JDK，请在下拉框选择。`
          : '未发现本地 JDK，可以手动指定 JDK Home。',
      );
  } catch (error) {
    fail(error);
  } finally {
    detecting.value = false;
  }
}
async function persist(next: AppConfig) {
  // 保存成功后再更新界面，避免磁盘写入失败时出现虚假的已保存状态。
  await api.persist(next);
  config.value = next;
}
async function saveProject(item: Project) {
  saving.value = true;
  try {
    const next = JSON.parse(JSON.stringify(config.value)) as AppConfig;
    const index = next.projects.findIndex((existing) => existing.id === item.id);
    if (index >= 0) next.projects[index] = item;
    else next.projects.push(item);
    await persist(next);
    selectedId.value = item.id;
    page.value = 'project';
    editing.value = null;
    notify('项目配置已保存');
    await refreshManifest();
  } catch (error) {
    fail(error);
  } finally {
    saving.value = false;
  }
}
async function saveSettings(settings: Settings) {
  saving.value = true;
  try {
    await persist({ ...config.value, settings });
    notify('工具设置已保存');
  } catch (error) {
    fail(error);
  } finally {
    saving.value = false;
  }
}
async function updatePreference(patch: Partial<Pick<Settings, 'theme' | 'openOutputAfterBuild'>>) {
  if (saving.value || loadFailed.value) return;
  saving.value = true;
  try {
    await persist({ ...config.value, settings: { ...config.value.settings, ...patch } });
  } catch (error) {
    fail(error);
  } finally {
    saving.value = false;
  }
}
function setTheme(theme: ThemeName) {
  return updatePreference({ theme });
}
async function setThemeFromMenu(event: Event) {
  const input = event.target as HTMLSelectElement;
  await setTheme(input.value as ThemeName);
  input.value = config.value.settings.theme;
}
async function setOpenOutput(event: Event) {
  const input = event.target as HTMLInputElement;
  await updatePreference({ openOutputAfterBuild: input.checked });
  // 保存失败时恢复勾选状态，避免界面与实际构建配置不一致。
  input.checked = config.value.settings.openOutputAfterBuild;
}
watch(() => config.value.settings.theme, applyTheme, { immediate: true, flush: 'sync' });
async function deleteProject() {
  if (!deleting.value || busy.value) return;
  saving.value = true;
  try {
    const id = deleting.value.id;
    await persist({
      ...config.value,
      projects: config.value.projects.filter((item) => item.id !== id),
    });
    if (selectedId.value === id) selectedId.value = config.value.projects[0]?.id || '';
    deleting.value = null;
    notify('已移除项目记录，本地文件保持不变');
  } catch (error) {
    fail(error);
  } finally {
    saving.value = false;
  }
}
function selectProject(item: Project) {
  selectedId.value = item.id;
  page.value = 'project';
  showProjectMenu.value = false;
}
function editProject() {
  if (project.value) editing.value = JSON.parse(JSON.stringify(project.value));
  showProjectMenu.value = false;
}
async function refreshManifest() {
  const item = project.value;
  const request = ++manifestRequest;
  manifest.value = null;
  versionName.value = '';
  versionCode.value = '';
  if (!item || item.type !== 'uniapp' || !item.platforms.includes('app') || !desktop) return;
  refreshing.value = true;
  try {
    const result = await api.manifest(JSON.parse(JSON.stringify(item)));
    if (request !== manifestRequest) return;
    manifest.value = result;
    versionName.value = result.versionName;
    versionCode.value = result.versionCode;
  } catch (error) {
    if (request === manifestRequest) fail(error);
  } finally {
    if (request === manifestRequest) refreshing.value = false;
  }
}
function handleEvent(event: TaskEvent) {
  if (event.kind === 'notice') notify(event.message);
  let task = tasks.value.find((item) => item.id === event.taskId);
  // 后端事件可能先于 invoke 返回，按任务 ID 接收，避免遗漏首条日志。
  if (!task) {
    task = {
      id: event.taskId,
      projectId: event.projectId,
      title: '本地任务',
      status: 'running',
      logs: [],
      artifacts: [],
    };
    tasks.value.unshift(task);
    task = tasks.value[0];
  }
  if (event.status) task.status = event.status;
  if (event.kind === 'artifact' && event.path && !task.artifacts.includes(event.path))
    task.artifacts.push(event.path);
  if (event.message)
    task.logs.push({
      time: new Date().toLocaleTimeString('zh-CN', { hour12: false }),
      message: event.message,
      kind: event.kind,
    });
  if (task.logs.length > 3000) task.logs.splice(0, task.logs.length - 3000);
  nextTick(() => {
    if (logElement.value) logElement.value.scrollTop = logElement.value.scrollHeight;
  });
}
async function execute(title: string, run: () => Promise<string>) {
  const item = project.value;
  if (!item || busy.value) return;
  submitting.value = true;
  try {
    const id = await run();
    const task = tasks.value.find((task) => task.id === id);
    if (task) task.title = title;
    else
      tasks.value.unshift({
        id,
        projectId: item.id,
        title,
        status: 'running',
        logs: [],
        artifacts: [],
      });
    selectedTaskId.value = id;
  } catch (error) {
    fail(error);
  } finally {
    submitting.value = false;
  }
}
async function build() {
  const item = project.value;
  if (!item) return;
  if (!versionName.value.trim() || !/^\d+$/.test(versionCode.value)) {
    notify('请填写版本名称和非负整数 versionCode。', true);
    return;
  }
  if (!currentAppId.value) {
    notify('请先在项目设置中配置对应平台的 AppID。', true);
    return;
  }
  await execute(modeLabels[buildMode.value], () =>
    api.startBuild(
      JSON.parse(JSON.stringify(item)),
      buildMode.value,
      versionName.value,
      versionCode.value,
    ),
  );
}
async function runAction(action: Action) {
  const item = project.value;
  if (item)
    await execute(action.name, () =>
      api.runAction(JSON.parse(JSON.stringify(item)), JSON.parse(JSON.stringify(action)), {
        ...config.value.settings,
      }),
    );
}
async function runStopAction(action: Action) {
  const stop = stopMenuAction(action);
  if (stop) await runAction(stop);
}
async function cancelTask() {
  if (!currentTask.value) return;
  try {
    await api.cancel(currentTask.value.id);
    notify('已请求取消，正在等待任务清理并恢复 AppID。');
  } catch (error) {
    fail(error);
  }
}
async function openPath(path: string) {
  try {
    await api.openPath(path);
  } catch (error) {
    fail(error);
  }
}
async function exportBackup() {
  try {
    if (await api.exportBackup(JSON.parse(JSON.stringify(config.value)))) notify('配置备份已导出');
  } catch (error) {
    fail(error);
  }
}
async function importBackup() {
  try {
    pendingImport.value = await api.importBackup();
    importMode.value = 'merge';
  } catch (error) {
    fail(error);
  }
}
async function applyImport() {
  if (!pendingImport.value || busy.value) return;
  saving.value = true;
  try {
    const current = JSON.parse(JSON.stringify(config.value));
    const incoming = JSON.parse(JSON.stringify(pendingImport.value));
    const next = importMode.value === 'merge' ? mergeConfig(current, incoming) : incoming;
    await persist(next);
    if (!next.projects.some((item: Project) => item.id === selectedId.value))
      selectedId.value = next.projects[0]?.id || '';
    pendingImport.value = null;
    notify('配置已导入。运行前请检查本机路径及导入的自定义命令。');
  } catch (error) {
    fail(error);
  } finally {
    saving.value = false;
  }
}
async function copyLogs() {
  try {
    await navigator.clipboard.writeText(
      currentTask.value?.logs.map((line) => `[${line.time}] ${line.message}`).join('\n') || '',
    );
    notify('日志已复制');
  } catch (error) {
    fail(error);
  }
}
watch(selectedId, () => {
  selectedPlatform.value = project.value?.platforms[0] || '';
  selectedTaskId.value = '';
  refreshManifest();
});
watch(
  () => project.value?.platforms.join(','),
  () => {
    if (!project.value?.platforms.includes(selectedPlatform.value))
      selectedPlatform.value = project.value?.platforms[0] || '';
  },
);
onMounted(async () => {
  if (desktop) {
    try {
      unlisten = await listen<TaskEvent>('task-event', (event) => handleEvent(event.payload));
      projectSelectionUnlisten = await listen<string>('open-project', async (event) => {
        if (!config.value.projects.some((item) => item.id === event.payload)) await load();
        const selected = config.value.projects.find((item) => item.id === event.payload);
        if (selected) selectProject(selected);
      });
    } catch (error) {
      fail(error);
    }
    try {
      const messages = await api.recover();
      if (messages.length) notify(messages.join('\n'), true);
    } catch (error) {
      fail(error);
    }
  }
  await load();
  if (desktop) await detect();
});
onUnmounted(() => {
  unlisten?.();
  projectSelectionUnlisten?.();
  if (noticeTimer) clearTimeout(noticeTimer);
});
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="#" aria-label="DevDock 工作台" @click.prevent="page = 'project'"
        ><img class="brand-mark" src="/devdock-icon.svg" alt="" />
        <span>DevDock<span class="brand-period">.</span></span></a
      >
      <div class="workspace-label">本地工作空间<span>01</span></div>
      <div class="search-box">
        <Search :size="15" /><input
          v-model="search"
          aria-label="搜索项目"
          placeholder="搜索项目…"
        /><span class="search-hint">⌕</span>
      </div>
      <div class="sidebar-section-title">
        <span>项目</span><span>{{ config.projects.length }}</span>
      </div>
      <nav class="project-list" aria-label="项目列表">
        <button
          v-for="item in filteredProjects"
          :key="item.id"
          :class="['project-item', { active: selectedId === item.id && page === 'project' }]"
          @click="selectProject(item)"
        >
          <span :class="['project-icon', item.type]"
            ><Layers v-if="item.type === 'uniapp'" :size="18" /><Terminal
              v-else-if="item.type === 'nodejs'"
              :size="18" /><Cpu v-else :size="18" /></span
          ><span class="project-name"
            >{{ item.name }}<small>{{ TYPE_LABELS[item.type] }}</small></span
          ><span
            v-if="tasks.some((task) => task.projectId === item.id && task.status === 'running')"
            class="running-dot"
          ></span
          ><ChevronRight
            v-else-if="selectedId === item.id && page === 'project'"
            :size="14"
            class="project-chevron"
          />
        </button>
        <p v-if="!filteredProjects.length" class="sidebar-empty">
          {{ search ? '没有匹配的项目' : '你的项目将在这里排列' }}
        </p>
      </nav>
      <button class="add-project" :disabled="loadFailed || loading" @click="editing = newProject()">
        <Plus :size="17" />添加项目
      </button>
      <div class="sidebar-bottom">
        <button
          :class="['settings-button', { active: page === 'settings' }]"
          :disabled="loadFailed"
          @click="page = 'settings'"
        >
          <Settings2 :size="18" />偏好设置<span>↗</span>
        </button>
        <div class="local-status">
          <span></span>{{ desktop ? '本地工作台' : '浏览器预览' }}<small>v0.1.0</small>
        </div>
      </div>
    </aside>
    <main class="main-area">
      <header class="topbar">
        <div class="breadcrumbs">
          <span>工作空间</span><ChevronRight :size="13" /><strong>{{
            page === 'settings' ? '偏好设置' : project?.name || '概览'
          }}</strong>
        </div>
        <div class="topbar-right">
          <label class="theme-toggle" title="切换主题">
            <Palette :size="14" />
            <select
              aria-label="切换主题"
              :value="config.settings.theme"
              :disabled="saving || loading || loadFailed"
              @change="setThemeFromMenu"
            >
              <option v-for="theme in THEME_OPTIONS" :key="theme.id" :value="theme.id">
                {{ theme.name }}
              </option>
            </select>
            <span class="theme-toggle-dot"></span>
          </label>
          <span v-if="!desktop" class="preview-badge">预览模式 · 本地调用不可用</span
          ><span class="platform-badge">macOS FIRST</span><span class="avatar">D.</span>
        </div>
      </header>
      <div v-if="notice" :class="['notice', { error: notice.error }]" role="status">
        <AlertCircle v-if="notice.error" :size="17" /><Check v-else :size="17" /><span>{{
          notice.message
        }}</span
        ><button class="icon-button" aria-label="关闭通知" @click="notice = null">
          <X :size="16" />
        </button>
      </div>
      <div v-if="loading" class="loading-state">
        <RefreshCw :size="24" class="spinning" />
        <p>正在载入工作空间…</p>
      </div>
      <div v-else-if="loadFailed" class="empty-state">
        <AlertCircle :size="36" />
        <h1>配置未能载入</h1>
        <p>为保护已有配置，已暂停编辑。请检查错误信息后重试。</p>
        <button class="button" @click="load">重新加载</button>
      </div>
      <SettingsPage
        v-else-if="page === 'settings'"
        :settings="config.settings"
        :tools="tools"
        :detecting="detecting"
        :saving="saving"
        :busy="busy"
        :project-count="config.projects.length"
        @save="saveSettings"
        @theme="setTheme"
        @detect="detect(true)"
        @export="exportBackup"
        @import="importBackup"
        @error="fail"
      />
      <div v-else-if="!project" class="welcome-page">
        <div class="welcome-kicker">
          <span class="tiny-line"></span>YOUR LOCAL DEVELOPMENT SPACE
        </div>
        <h1>项目就位，<br />开发开始<span>.</span></h1>
        <p class="welcome-description">
          把项目、构建和常用命令放在一起。<br />少一点切换，多一点专注。
        </p>
        <button class="button primary welcome-add" @click="editing = newProject()">
          <Plus :size="18" />添加第一个项目<ArrowUpRight :size="18" />
        </button>
        <div class="welcome-features">
          <div>
            <div class="feature-icon"><Package :size="23" /></div>
            <span class="mono">01 / BUILD</span>
            <h3>App 构建</h3>
            <p>Android APK 与双平台 WGT<br />版本和 AppID，统一管理。</p>
          </div>
          <div>
            <div class="feature-icon"><Layers :size="23" /></div>
            <span class="mono">02 / LAUNCH</span>
            <h3>小程序运行</h3>
            <p>连接 HBuilderX 菜单<br />一键触发熟悉的工作流。</p>
          </div>
          <div>
            <div class="feature-icon"><Terminal :size="23" /></div>
            <span class="mono">03 / DEVELOP</span>
            <h3>终端命令</h3>
            <p>用你喜欢的终端<br />启动每个项目的开发环境。</p>
          </div>
        </div>
        <div class="welcome-footer">
          <span class="local-dot"></span>源码留在本机，工具各司其职。<button
            class="text-button"
            @click="page = 'settings'"
          >
            配置开发工具 <ArrowUpRight :size="14" />
          </button>
        </div>
      </div>
      <section v-else class="project-page">
        <div class="page-heading">
          <div>
            <div class="eyebrow">
              PROJECT WORKSPACE <span>/ {{ TYPE_LABELS[project.type] }}</span>
            </div>
            <h1>{{ project.name }}<span class="heading-dot">.</span></h1>
            <button class="project-path" @click="openPath(project.path)">
              <FolderOpen :size="14" />{{ project.path }}<ArrowUpRight :size="13" />
            </button>
            <GitBranch :path="project.path" />
          </div>
          <div class="heading-actions">
            <button class="button" :disabled="busy" @click="editProject">
              <Pencil :size="15" />编辑项目
            </button>
            <div class="dropdown-wrap">
              <button
                class="icon-button more-button"
                aria-label="更多项目操作"
                :aria-expanded="showProjectMenu"
                @click="showProjectMenu = !showProjectMenu"
              >
                <MoreHorizontal :size="20" />
              </button>
              <div v-if="showProjectMenu" class="dropdown">
                <button
                  @click="
                    openPath(project.path);
                    showProjectMenu = false;
                  "
                >
                  <FolderOpen :size="15" />打开目录</button
                ><button
                  class="danger-text"
                  :disabled="busy"
                  @click="
                    deleting = project;
                    showProjectMenu = false;
                  "
                >
                  <Trash2 :size="15" />移除项目
                </button>
              </div>
            </div>
          </div>
        </div>
        <nav v-if="project.platforms.length" class="tabs platform-tabs" aria-label="项目平台">
          <button
            v-for="platform in project.platforms"
            :key="platform"
            :class="{ active: selectedPlatform === platform }"
            @click="selectedPlatform = platform"
          >
            <Smartphone v-if="platform === 'app'" :size="16" /><Terminal
              v-else-if="platform === 'pc'"
              :size="16"
            /><Layers v-else :size="16" />{{ PLATFORM_LABELS[platform] || platform }}
          </button>
        </nav>
        <div v-if="project.type === 'nodejs'" class="node-runtime-summary">
          <span
            ><Terminal :size="15" />Node.js
            <strong>{{ project.nodeVersion || '终端默认版本' }}</strong
            ><span v-if="project.nodeVersion" class="tag">nvm</span></span
          >
          <span v-if="project.nvmDir" class="runtime-path">{{ project.nvmDir }}</span>
          <button class="text-button" :disabled="busy" @click="editProject">
            修改版本 <ArrowUpRight :size="13" />
          </button>
        </div>
        <template v-if="project.type === 'uniapp' && selectedPlatform === 'app'">
          <div class="build-layout">
            <section class="panel build-panel">
              <div class="panel-heading">
                <div>
                  <div class="eyebrow">BUILD CONFIGURATION</div>
                  <h2>准备你的下一次发布</h2>
                </div>
                <Package :size="24" class="muted-icon" />
              </div>
              <span class="field-label">构建目标</span>
              <div class="build-targets">
                <button
                  v-for="mode in ['apk', 'wgt-android', 'wgt-ios'] as BuildMode[]"
                  :key="mode"
                  :class="['build-target', { selected: buildMode === mode }]"
                  :disabled="busy"
                  :aria-pressed="buildMode === mode"
                  @click="buildMode = mode"
                >
                  <div class="target-top">
                    <Package v-if="mode === 'apk'" :size="19" /><FileBox v-else :size="19" /><span
                      class="radio-dot"
                    ></span>
                  </div>
                  <strong>{{
                    mode === 'apk' ? 'Android' : mode === 'wgt-android' ? 'Android' : 'iOS'
                  }}</strong
                  ><span>{{ mode === 'apk' ? 'APK 安装包' : 'WGT 热更新' }}</span>
                </button>
              </div>
              <div class="version-heading">
                <span class="field-label">应用版本</span
                ><button
                  class="text-button"
                  :disabled="refreshing || busy || !desktop"
                  @click="refreshManifest"
                >
                  <RefreshCw :size="13" :class="{ spinning: refreshing }" />重新读取
                </button>
              </div>
              <div class="form-grid">
                <label
                  >versionName<input
                    v-model="versionName"
                    :disabled="busy"
                    placeholder="例如：3.4.8" /></label
                ><label
                  >versionCode<input
                    v-model="versionCode"
                    :disabled="busy"
                    placeholder="例如：34800"
                    inputmode="numeric"
                /></label>
              </div>
              <p class="field-help">版本写入源码；构建期间的 AppID 切换会在任务结束后恢复。</p>
              <div class="appid-summary">
                <span>目标 AppID</span
                ><code :class="{ missing: !currentAppId }">{{ currentAppId || '尚未配置' }}</code
                ><button
                  v-if="!currentAppId"
                  class="text-button"
                  :disabled="busy"
                  @click="editProject"
                >
                  去设置 <ArrowUpRight :size="13" />
                </button>
              </div>
              <div class="build-footer">
                <span><span class="local-dot"></span>{{ busy ? '任务执行中' : '本机执行' }}</span>
                <div class="build-footer-actions">
                  <label class="open-output-option"
                    ><input
                      type="checkbox"
                      :checked="config.settings.openOutputAfterBuild"
                      :disabled="saving || busy"
                      @change="setOpenOutput"
                    />构建完打开输出目录</label
                  >
                  <button
                    class="button primary build-button"
                    :disabled="busy || saving || refreshing || !desktop"
                    @click="build"
                  >
                    <Play :size="16" fill="currentColor" />{{
                      submitting ? '正在启动…' : '开始构建'
                    }}
                  </button>
                </div>
              </div>
            </section>
            <aside class="build-aside">
              <section class="context-card">
                <div class="eyebrow">ENVIRONMENT</div>
                <h3>构建环境</h3>
                <dl>
                  <dt>构建工具</dt>
                  <dd>HBuilderX <span class="tag">菜单自动化</span></dd>
                  <template v-if="buildMode === 'apk'"
                    ><dt>Android 工程</dt>
                    <dd class="path-value">{{ project.androidProjectPath || '尚未配置' }}</dd>
                    <dt>模块 / Gradle 任务</dt>
                    <dd class="mono">{{ project.androidModule }}<br />{{ project.gradleTask }}</dd>
                    <dt>JDK</dt>
                    <dd class="path-value">
                      {{ project.jdkPath || config.settings.jdkPath || '系统 JAVA_HOME' }}
                    </dd></template
                  ><template v-else
                    ><dt>产物格式</dt>
                    <dd>.wgt 热更新资源包</dd>
                    <dt>平台</dt>
                    <dd>{{ buildMode === 'wgt-ios' ? 'iOS' : 'Android' }}</dd></template
                  >
                  <dt>当前源码 AppID</dt>
                  <dd class="mono">{{ manifest?.appid || '未读取' }}</dd>
                </dl>
                <button class="text-button" :disabled="busy" @click="editProject">
                  调整项目配置 <ArrowUpRight :size="14" />
                </button>
              </section>
              <div class="quiet-note">
                <span>↳</span>
                <p>
                  {{
                    buildMode === 'apk'
                      ? '先生成本地 App 资源，再调用原生工程完成 APK 构建。'
                      : 'WGT 只包含更新资源。原生依赖变更仍需要发布新的安装包。'
                  }}
                </p>
              </div>
            </aside>
          </div>
        </template>
        <section v-else-if="project.type === 'backend'" class="panel backend-placeholder">
          <Cpu :size="32" />
          <h2>为后端项目留一个位置</h2>
          <p>已保存项目目录。运行与发布能力将在后续版本提供。</p>
          <button class="button" @click="openPath(project.path)">
            <FolderOpen :size="16" />打开项目目录
          </button>
        </section>
        <section
          v-if="project.type !== 'backend' && (selectedPlatform !== 'app' || actions.length)"
          class="actions-section"
        >
          <div class="section-heading">
            <div>
              <div v-if="project.type !== 'nodejs' && selectedPlatform === 'app'" class="eyebrow">
                PROJECT ACTIONS
              </div>
              <h2>
                {{
                  project.type === 'nodejs'
                    ? '运行命令'
                    : selectedPlatform === 'app'
                      ? '更多操作'
                      : '运行与发行'
                }}
              </h2>
              <p>
                {{
                  project.type === 'nodejs'
                    ? '在所选终端中启动，日志与进程由终端管理。'
                    : '切换至目标项目后，通过 HBuilderX 菜单触发操作。'
                }}
              </p>
            </div>
            <button class="button" :disabled="busy" @click="editProject">
              <Plus :size="15" />配置操作
            </button>
          </div>
          <div v-if="!actions.length" class="panel empty-actions">
            <Terminal :size="27" />
            <h3>还没有{{ PLATFORM_LABELS[selectedPlatform] }}操作</h3>
            <p>添加运行命令或菜单路径，建立项目的常用工作流。</p>
            <button class="button" :disabled="busy" @click="editProject">
              添加操作 <ArrowUpRight :size="14" />
            </button>
          </div>
          <div
            v-else-if="project.type === 'nodejs' || selectedPlatform !== 'app'"
            class="command-list"
            role="list"
            aria-label="项目操作命令"
          >
            <article v-for="action in actions" :key="action.id" class="command-row" role="listitem">
              <div class="command-name" :title="action.name">
                <Terminal v-if="action.kind === 'shell'" :size="15" /><Layers
                  v-else
                  :size="15"
                /><span>{{ action.name }}</span>
              </div>
              <code
                :title="action.kind === 'shell' ? action.command : action.menuPath.join(' → ')"
                >{{ action.kind === 'shell' ? action.command : action.menuPath.join(' → ') }}</code
              >
              <div class="command-row-actions">
                <button
                  class="button small"
                  :disabled="busy || !desktop"
                  :aria-label="`执行 ${action.name}`"
                  @click="runAction(action)"
                >
                  <Play :size="13" />执行
                </button>
                <button
                  v-if="stopMenuAction(action)"
                  class="button small stop-command"
                  :disabled="busy || !desktop"
                  :aria-label="`停止 ${action.name}`"
                  title="停止当前项目的运行任务"
                  @click="runStopAction(action)"
                >
                  <Square :size="11" />停止
                </button>
              </div>
            </article>
          </div>
          <div v-else class="action-grid">
            <article v-for="action in actions" :key="action.id" class="action-card">
              <div class="action-card-top">
                <span class="action-symbol"
                  ><Terminal v-if="action.kind === 'shell'" :size="19" /><Layers
                    v-else
                    :size="19" /></span
                ><span class="tag">{{ action.kind === 'shell' ? '终端命令' : 'HBuilderX' }}</span>
              </div>
              <h3>{{ action.name }}</h3>
              <code>{{
                action.kind === 'shell' ? action.command : action.menuPath.join(' → ')
              }}</code
              ><button class="button" :disabled="busy || !desktop" @click="runAction(action)">
                <Play :size="14" />执行操作 <ArrowUpRight :size="14" />
              </button>
            </article>
          </div>
        </section>
        <section class="task-panel">
          <header class="task-header">
            <div class="task-title">
              <Terminal :size="16" />
              <h3>任务输出</h3>
              <span v-if="currentTask" :class="['status-pill', currentTask.status]">{{
                statusLabels[currentTask.status]
              }}</span>
            </div>
            <div class="button-row">
              <select
                v-if="projectTasks.length > 1"
                v-model="selectedTaskId"
                class="task-select"
                aria-label="选择任务"
              >
                <option v-for="task in projectTasks" :key="task.id" :value="task.id">
                  {{ task.title }} · {{ statusLabels[task.status] }}
                </option></select
              ><button
                v-if="currentTask"
                class="icon-button"
                aria-label="复制日志"
                @click="copyLogs"
              >
                <Copy :size="15" /></button
              ><button
                v-if="currentTask?.status === 'running'"
                class="button small stop-button"
                @click="cancelTask"
              >
                <Square :size="11" fill="currentColor" />取消</button
              ><span v-if="!currentTask" class="mono log-caption">OUTPUT / LOCAL</span>
            </div>
          </header>
          <div
            ref="logElement"
            class="log-content"
            :class="{ 'has-logs': currentTask?.logs.length }"
            role="log"
            aria-live="polite"
          >
            <template v-if="currentTask?.logs.length"
              ><div v-for="(line, index) in currentTask.logs" :key="index" class="log-line">
                <time>{{ line.time }}</time
                ><span
                  :class="{ 'log-status': line.kind === 'status' || line.kind === 'notice' }"
                  >{{ line.message }}</span
                >
              </div></template
            >
            <div v-else class="log-placeholder">
              <span class="terminal-prompt">❯</span>
              <div>一切就绪，等待任务开始<span>构建过程与执行结果将在这里显示。</span></div>
              <span class="cursor-block"></span>
            </div>
          </div>
          <div v-if="currentTask?.artifacts.length" class="artifact-list">
            <button
              v-for="path in currentTask.artifacts"
              :key="path"
              class="artifact"
              title="在 Finder 中显示"
              :aria-label="`在 Finder 中显示 ${path}`"
              @click="openPath(path)"
            >
              <FileBox :size="17" /><span>{{ path }}</span
              ><FolderOpen :size="16" />
            </button>
          </div>
        </section>
        <footer class="workspace-footer">
          <span>每一次构建，都有迹可循。</span><span>DevDock / LOCAL FIRST</span>
        </footer>
      </section>
    </main>
    <ProjectDialog
      v-if="editing"
      :project="editing"
      :existing="config.projects"
      :tools="tools"
      :hbuilderx-path="config.settings.hbuilderxPath"
      :saving="saving"
      :detecting="detecting"
      @detect="detect(true)"
      @close="editing = null"
      @save="saveProject"
    />
    <div v-if="deleting" class="modal-backdrop" @click.self="!saving && (deleting = null)">
      <section
        class="modal confirm-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-title"
      >
        <div class="confirm-symbol danger-text"><Trash2 :size="25" /></div>
        <h2 id="delete-title">移除「{{ deleting.name }}」？</h2>
        <p>仅移除工作台中的项目记录，本地源码和构建产物不会被删除。</p>
        <div class="modal-footer">
          <button class="button" :disabled="saving" @click="deleting = null">保留项目</button
          ><button class="button danger" :disabled="saving || busy" @click="deleteProject">
            移除记录
          </button>
        </div>
      </section>
    </div>
    <div
      v-if="pendingImport"
      class="modal-backdrop"
      @click.self="!saving && (pendingImport = null)"
    >
      <section
        class="modal confirm-modal import-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-title"
      >
        <div class="confirm-symbol"><Download :size="26" /></div>
        <h2 id="import-title">恢复工作台配置</h2>
        <p>
          备份已通过格式校验，包含
          {{ pendingImport.projects.length }} 个项目。导入不会执行任何命令。
        </p>
        <label class="import-choice" :class="{ selected: importMode === 'merge' }"
          ><input v-model="importMode" type="radio" value="merge" /><span
            ><strong>合并项目</strong
            ><small>相同 ID 或目录的项目由备份更新，保留其他项目及本机工具设置。</small></span
          ></label
        ><label class="import-choice" :class="{ selected: importMode === 'replace' }"
          ><input v-model="importMode" type="radio" value="replace" /><span
            ><strong>替换全部配置</strong
            ><small>用备份替换全部项目和工具设置。建议先导出当前配置。</small></span
          ></label
        >
        <p class="field-help">路径可能属于另一台设备；导入后请检查路径及自定义命令。</p>
        <div class="modal-footer">
          <button class="button" :disabled="saving" @click="pendingImport = null">取消</button
          ><button class="button primary" :disabled="saving || busy" @click="applyImport">
            {{ saving ? '导入中…' : '确认导入' }}
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
