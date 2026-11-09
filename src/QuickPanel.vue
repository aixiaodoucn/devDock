<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  ArrowUpRight,
  ChevronDown,
  Search,
  RefreshCw,
  Terminal,
  Layers,
  Play,
  FolderGit2,
  AlertCircle,
  Square,
} from 'lucide-vue-next';
import { api, desktop } from './api';
import { applyTheme } from './theme';
import { stopMenuAction } from './actions';
import {
  defaultConfig,
  TYPE_LABELS,
  PLATFORM_LABELS,
  type Project,
  type Action,
  type TaskEvent,
} from './types';

const config = ref(defaultConfig());
const query = ref('');
const loading = ref(true);
const loadError = ref('');
const expanded = ref<string[]>([]);
const launching = ref(false);
const eventsReady = ref(!desktop);
const status = ref('');
const statusError = ref(false);
const searchInput = ref<HTMLInputElement>();
const activeProjectId = ref('');
let activeTaskId = '';
let initialized = false;
let requestId = 0;
let disposed = false;
const unlisteners: UnlistenFn[] = [];
const taskStates = new Map<string, TaskEvent>();
const taskNotices = new Map<string, string>();
const needle = computed(() => query.value.trim().toLowerCase());
function matchesAction(action: Action) {
  return `${action.name} ${action.command} ${action.menuPath.join(' ')}`
    .toLowerCase()
    .includes(needle.value);
}
const projects = computed(() =>
  config.value.projects.filter(
    (project) =>
      !needle.value ||
      project.name.toLowerCase().includes(needle.value) ||
      project.commands.some(matchesAction),
  ),
);
function projectActions(project: Project) {
  return !needle.value || project.name.toLowerCase().includes(needle.value)
    ? project.commands
    : project.commands.filter(matchesAction);
}
function toggleProject(id: string) {
  expanded.value = expanded.value.includes(id)
    ? expanded.value.filter((item) => item !== id)
    : [...expanded.value, id];
}
function isExpanded(id: string) {
  return !!needle.value || expanded.value.includes(id);
}
function actionText(action: Action) {
  return action.kind === 'shell' ? action.command : action.menuPath.join(' → ');
}
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
async function refresh(focus = false) {
  const request = ++requestId;
  loading.value = true;
  try {
    const next = await api.load();
    if (disposed || request !== requestId) return;
    config.value = next;
    loadError.value = '';
    if (!initialized) {
      expanded.value = next.projects[0] ? [next.projects[0].id] : [];
      initialized = true;
    } else
      expanded.value = expanded.value.filter((id) =>
        next.projects.some((project) => project.id === id),
      );
  } catch (error) {
    if (!disposed && request === requestId) loadError.value = message(error);
  } finally {
    if (!disposed && request === requestId) {
      loading.value = false;
      if (focus) {
        await nextTick();
        searchInput.value?.focus();
      }
    }
  }
}
async function openMain(projectId?: string) {
  try {
    await api.showMain(projectId);
  } catch (error) {
    status.value = message(error);
    statusError.value = true;
  }
}
async function hide() {
  if (desktop) {
    try {
      await api.hideQuickPanel();
    } catch (error) {
      status.value = message(error);
      statusError.value = true;
    }
  }
}
function applyTask(event: TaskEvent) {
  if (!event.status) return;
  launching.value = event.status === 'running';
  statusError.value = event.status === 'error';
  status.value =
    event.status === 'triggered'
      ? taskNotices.get(event.taskId) || '已触发，请在对应工具查看运行日志。'
      : event.status === 'running'
        ? '正在启动…'
        : event.status === 'success'
          ? '任务已完成。'
          : event.message;
}
function handleTask(event: TaskEvent) {
  if (event.kind === 'notice') {
    taskNotices.set(event.taskId, event.message);
    if (event.taskId === activeTaskId) {
      status.value = event.message;
      statusError.value = false;
    }
    return;
  }
  if (event.kind !== 'status') return;
  // 任务事件可能先于 invoke 返回，先缓存状态，避免漏掉快速失败。
  taskStates.set(event.taskId, event);
  if (taskStates.size > 50) {
    const oldest = taskStates.keys().next().value!;
    taskStates.delete(oldest);
    taskNotices.delete(oldest);
  }
  if (event.taskId === activeTaskId) applyTask(event);
}
async function run(project: Project, action: Action) {
  if (launching.value || loading.value || !eventsReady.value) return;
  launching.value = true;
  statusError.value = false;
  status.value = `正在启动「${action.name}」…`;
  activeProjectId.value = project.id;
  activeTaskId = '';
  try {
    activeTaskId = await api.runAction(
      JSON.parse(JSON.stringify(project)),
      JSON.parse(JSON.stringify(action)),
      JSON.parse(JSON.stringify(config.value.settings)),
    );
    const event = taskStates.get(activeTaskId);
    if (event) applyTask(event);
  } catch (error) {
    launching.value = false;
    status.value = message(error);
    statusError.value = true;
  }
}
async function runStop(project: Project, action: Action) {
  const stop = stopMenuAction(action);
  if (stop) await run(project, stop);
}
watch(() => config.value.settings.theme, applyTheme, { immediate: true, flush: 'sync' });
onMounted(async () => {
  try {
    if (desktop) {
      for (const [event, handler] of [
        [
          'quick-panel-opened',
          () => {
            query.value = '';
            refresh(true);
          },
        ],
        ['config-updated', () => refresh()],
      ] as const) {
        const stop = await listen(event, handler);
        if (disposed) stop();
        else unlisteners.push(stop);
      }
      const stop = await listen<TaskEvent>('task-event', (event) => handleTask(event.payload));
      if (disposed) stop();
      else unlisteners.push(stop);
      if (!disposed) eventsReady.value = true;
    }
  } catch (error) {
    status.value = `快捷状态监听失败，请在工作台执行：${message(error)}`;
    statusError.value = true;
  }
  // 隐藏预创建的弹框不主动请求输入焦点，仅收到显示事件后再聚焦搜索。
  if (!disposed) await refresh();
});
onUnmounted(() => {
  disposed = true;
  requestId++;
  unlisteners.forEach((stop) => stop());
});
</script>

<template>
  <div class="quick-panel" @keydown.esc.prevent="hide">
    <header class="quick-header">
      <div class="quick-brand">
        <img src="/devdock-icon.svg" alt="" />
        <div>
          <strong>DevDock<span>.</span></strong
          ><small>项目快捷入口</small>
        </div>
      </div>
      <button class="button small" :disabled="!desktop" @click="openMain()">
        打开 DevDock <ArrowUpRight :size="13" />
      </button>
    </header>
    <div class="quick-toolbar">
      <label class="quick-search"
        ><Search :size="14" /><input
          ref="searchInput"
          v-model="query"
          placeholder="搜索项目或命令"
          aria-label="搜索项目或命令"
        /><kbd>Esc</kbd></label
      >
      <div class="quick-section-label">
        <span
          >我的项目 <small>{{ config.projects.length }}</small></span
        ><button class="icon-button" :disabled="loading" aria-label="刷新项目" @click="refresh()">
          <RefreshCw :size="13" :class="{ spinning: loading }" />
        </button>
      </div>
    </div>
    <main class="quick-projects" aria-label="快捷项目列表">
      <div v-if="loadError" class="quick-empty error">
        <AlertCircle :size="26" />
        <h2>无法读取项目</h2>
        <p>{{ loadError }}</p>
        <button class="button small" @click="refresh()">重新读取</button>
      </div>
      <div v-else-if="loading && !initialized" class="quick-empty">
        <RefreshCw :size="23" class="spinning" />
        <p>正在读取项目…</p>
      </div>
      <div v-else-if="!projects.length" class="quick-empty">
        <FolderGit2 :size="28" />
        <h2>{{ needle ? '没有匹配的项目或命令' : '还没有配置项目' }}</h2>
        <p>
          {{ needle ? '试试项目名称或脚本名称。' : '在工作台添加项目后，即可从这里快速启动。' }}
        </p>
        <button v-if="!needle" class="button small" :disabled="!desktop" @click="openMain()">
          打开 DevDock 添加项目
        </button>
      </div>
      <section v-for="project in loadError ? [] : projects" :key="project.id" class="quick-project">
        <div class="quick-project-header">
          <button
            class="quick-project-toggle"
            :aria-expanded="isExpanded(project.id)"
            @click="toggleProject(project.id)"
          >
            <span class="quick-project-icon"
              ><Terminal v-if="project.type === 'nodejs'" :size="15" /><Layers v-else :size="15"
            /></span>
            <span class="quick-project-title"
              ><strong>{{ project.name }}</strong
              ><small
                >{{ TYPE_LABELS[project.type]
                }}<template v-if="project.type === 'nodejs' && project.nodeVersion">
                  · {{ project.nodeVersion }}</template
                ></small
              ></span
            >
            <ChevronDown :size="14" :class="{ expanded: isExpanded(project.id) }" />
          </button>
          <button
            class="icon-button quick-open-project"
            :disabled="!desktop"
            :aria-label="`打开项目 ${project.name}`"
            title="在工作台打开项目"
            @click="openMain(project.id)"
          >
            <ArrowUpRight :size="14" />
          </button>
        </div>
        <div v-if="isExpanded(project.id)" class="quick-actions">
          <div v-for="action in projectActions(project)" :key="action.id" class="quick-action-line">
            <button
              class="quick-action"
              :disabled="launching || loading || !desktop || !eventsReady"
              :title="actionText(action)"
              :aria-label="`执行 ${project.name}：${action.name}`"
              @click="run(project, action)"
            >
              <span class="quick-action-copy"
                ><strong>{{ action.name }}</strong
                ><code>{{ actionText(action) }}</code></span
              ><span class="quick-platform">{{
                PLATFORM_LABELS[action.platform] || action.platform
              }}</span
              ><Play :size="12" />
            </button>
            <button
              v-if="stopMenuAction(action)"
              class="icon-button quick-stop"
              :disabled="launching || loading || !desktop || !eventsReady"
              :aria-label="`停止 ${project.name}：${action.name}`"
              title="停止当前项目的运行任务"
              @click="runStop(project, action)"
            >
              <Square :size="12" />
            </button>
          </div>
          <div v-if="!projectActions(project).length" class="quick-no-actions">
            <span>{{ project.type === 'backend' ? '后端操作待后续支持' : '暂无快捷命令' }}</span
            ><button class="text-button" :disabled="!desktop" @click="openMain(project.id)">
              前往项目操作区 <ArrowUpRight :size="12" />
            </button>
          </div>
        </div>
      </section>
    </main>
    <footer class="quick-footer">
      <div v-if="status" :class="['quick-status', { error: statusError }]" role="status">
        <span>{{ status }}</span
        ><button
          v-if="activeProjectId"
          class="text-button"
          :disabled="!desktop"
          @click="openMain(activeProjectId)"
        >
          查看任务 <ArrowUpRight :size="11" />
        </button>
      </div>
      <div class="quick-footer-note">
        <span
          ><span class="local-dot"></span
          >{{ desktop ? '本地运行 · 日志在对应工具查看' : '浏览器预览 · 本地执行不可用' }}</span
        ><span><kbd>Esc</kbd> 关闭</span>
      </div>
    </footer>
  </div>
</template>
