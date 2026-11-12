<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api, desktop } from '../api';
import { mapBackendModules, scmProjectId } from '../backend-mapping';
import type {
  Project,
  BackendGitPlan,
  BackendGitOperation,
  BackendRepositoryStatus,
  ScmModule,
  ScmProject,
  ScmStatus,
} from '../types';
import ScmConnection from './ScmConnection.vue';
const props = defineProps<{ project: Project; saving: boolean; busy: boolean }>();
const emit = defineEmits<{
  save: [project: Project];
  edit: [];
  task: [taskId: string, title: string];
}>();
const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));
const draft = ref<Project>(copy(props.project));
const statuses = ref<BackendRepositoryStatus[]>([]);
const selected = ref<string[]>(props.project.subProjects.map((item) => item.id));
const error = ref('');
const notice = ref('');
const activity = ref('');
const waiting = ref('');
const plan = ref<BackendGitPlan | null>(null);
const operation = ref<BackendGitOperation | null>(null);
const history = ref<BackendGitOperation[]>([]);
const push = ref(false);
const cleanConfirmed = ref<Record<string, boolean>>({});
const connected = ref(false);
const sessionExpired = ref(false);
const projects = ref<ScmProject[]>([]);
const development = ref<ScmModule[]>([]);
const testing = ref<ScmModule[]>([]);
let generation = 0;
let stateSequence = 0;
let repositorySequence = 0;
let historySequence = 0;
let disposed = false;
let unlisten: UnlistenFn | undefined;
const signature = (value: Project) =>
  JSON.stringify([value.subProjects, value.scmDevelopmentProjectId, value.scmTestProjectId]);
let persisted = copy(props.project);
function mergeDraft(project: Project): Project {
  const merged = copy(project);
  for (const field of ['scmDevelopmentProjectId', 'scmTestProjectId'] as const) {
    if (project[field] === persisted[field]) merged[field] = draft.value[field];
  }
  merged.subProjects = merged.subProjects.map((repo) => {
    const previous = persisted.subProjects.find((item) => item.id === repo.id);
    const local = draft.value.subProjects.find((item) => item.id === repo.id);
    // 路径或外部 remote 改变代表仓库身份改变，不能沿用旧仓库的草稿映射。
    if (!previous || !local || previous.path !== repo.path || previous.remote !== repo.remote)
      return repo;
    for (const field of [
      'remote',
      'scmGitId',
      'scmModuleName',
      'developmentBranch',
      'testBranch',
    ] as const) {
      if (repo[field] === previous[field]) repo[field] = local[field];
    }
    return repo;
  });
  return merged;
}
function testProjectId(): string | undefined {
  if (!draft.value.scmTestProjectId.trim()) return undefined;
  try {
    return scmProjectId(draft.value.scmTestProjectId);
  } catch {
    throw new Error('测试 SCM 项目 ID 无效：请输入纯数字 SCM 项目 ID 或含项目 ID 的完整链接。');
  }
}
const dirty = computed(() => signature(draft.value) !== signature(props.project));
const running = computed(
  () =>
    operation.value?.status === 'running' ||
    operation.value?.status === 'aborting' ||
    history.value.some((item) => item.status === 'running' || item.status === 'aborting'),
);
const locked = computed(
  () => props.saving || props.busy || !!activity.value || running.value || !!waiting.value,
);
const canPrepare = computed(
  () =>
    desktop &&
    !locked.value &&
    !dirty.value &&
    selected.value.length > 0 &&
    selected.value.every((id) => {
      const repo = draft.value.subProjects.find((item) => item.id === id);
      return !!repo?.remote.trim() && !!repo.developmentBranch.trim();
    }),
);
const statusFor = (id: string) => statuses.value.find((item) => item.id === id);
function valid(token: number) {
  return !disposed && token === generation;
}
function failure(reason: unknown) {
  const message = String(reason instanceof Error ? reason.message : reason);
  if (/sessionExpired|session.expired|会话.*过期|unauthorized|\b401\b/i.test(message)) {
    connected.value = false;
    sessionExpired.value = true;
    error.value = 'SCM 会话已过期，请重新登录。';
  } else error.value = message;
}
async function perform(label: string, action: (token: number) => Promise<void>) {
  if (!desktop || locked.value) return;
  const token = generation;
  activity.value = label;
  error.value = '';
  try {
    await action(token);
  } catch (reason) {
    if (valid(token)) failure(reason);
  } finally {
    if (valid(token)) activity.value = '';
  }
}
async function refreshRepositories(token = generation) {
  if (!desktop) return;
  const sequence = ++repositorySequence;
  try {
    const result = await api.backendRepositoryStatuses(copy(props.project.subProjects));
    if (valid(token) && sequence === repositorySequence) {
      statuses.value = result;
      // 普通目录及读取失败的仓库仍展示，但不进入批量 Git 操作。
      const unavailable = new Set(result.filter((item) => item.error).map((item) => item.id));
      selected.value = selected.value.filter((id) => !unavailable.has(id));
    }
  } catch (reason) {
    if (valid(token) && sequence === repositorySequence) failure(reason);
  }
}
async function refreshHistory(token = generation) {
  if (!desktop) return;
  const sequence = ++historySequence;
  try {
    const result = await api.backendGitOperations(props.project.id);
    if (!valid(token) || sequence !== historySequence) return;
    history.value = result;
    if (!operation.value)
      operation.value =
        result.find((item) => ['running', 'aborting'].includes(item.status)) ??
        result.find((item) => item.status !== 'success' && item.status !== 'cancelled') ??
        null;
  } catch (reason) {
    if (valid(token) && sequence === historySequence) failure(reason);
  }
}
async function refreshState(planId: string) {
  const token = generation;
  const sequence = ++stateSequence;
  try {
    const state = await api.backendGitState(planId);
    if (!valid(token) || sequence !== stateSequence || operation.value?.planId !== planId) return;
    operation.value = state;
    ++historySequence;
    history.value = [...history.value.filter((item) => item.planId !== planId), state];
    if (!['running', 'aborting'].includes(state.status)) void refreshRepositories(token);
  } catch (reason) {
    if (valid(token) && sequence === stateSequence) failure(reason);
  }
}
function save() {
  if (locked.value || !dirty.value) return;
  try {
    const snapshot = {
      ...copy(props.project),
      subProjects: copy(draft.value.subProjects),
      scmDevelopmentProjectId: draft.value.scmDevelopmentProjectId.trim()
        ? scmProjectId(draft.value.scmDevelopmentProjectId)
        : '',
      scmTestProjectId: testProjectId() ?? '',
    };
    // 保存与回执使用同一规范快照，链接只作为输入，不能进入项目配置。
    draft.value = copy(snapshot);
    plan.value = null;
    waiting.value = signature(snapshot);
    notice.value = '';
    error.value = '';
    emit('save', snapshot);
  } catch (reason) {
    failure(reason);
  }
}
watch(
  () => props.saving,
  (value, previous) => {
    if (previous && !value && waiting.value && signature(props.project) !== waiting.value) {
      waiting.value = '';
      error.value = '保存未完成，请检查提示后重试。';
    }
  },
);
watch(
  () => props.project,
  (project) => {
    const changed = signature(project) !== signature(persisted);
    // 同一项目的映射从外部更新时，正在准备的旧计划也必须失效。
    if (project.id === persisted.id && changed) {
      ++generation;
      ++stateSequence;
      plan.value = null;
      activity.value = '';
    }
    if (project.id !== persisted.id) {
      ++generation;
      ++stateSequence;
      draft.value = copy(project);
      selected.value = project.subProjects.map((item) => item.id);
      statuses.value = [];
      plan.value = null;
      operation.value = null;
      history.value = [];
      development.value = [];
      testing.value = [];
      push.value = false;
      cleanConfirmed.value = {};
      waiting.value = '';
      notice.value = '';
      error.value = '';
      activity.value = '';
      void refreshRepositories();
      void refreshHistory();
    } else if (waiting.value && signature(project) === waiting.value) {
      draft.value = copy(project);
      waiting.value = '';
      notice.value = '映射已保存';
    } else if (changed) {
      draft.value = mergeDraft(project);
    }
    if (changed && project.id === persisted.id) {
      const previousIds = new Set(persisted.subProjects.map((repo) => repo.id));
      // 新仓库默认选中，旧仓库保留用户选择；移除已删除或读取失败的仓库。
      selected.value = project.subProjects
        .filter((repo) => selected.value.includes(repo.id) || !previousIds.has(repo.id))
        .map((repo) => repo.id);
      statuses.value = statuses.value.filter((status) => {
        const previous = persisted.subProjects.find((repo) => repo.id === status.id);
        const current = project.subProjects.find((repo) => repo.id === status.id);
        return (
          previous &&
          current &&
          previous.path === current.path &&
          previous.remote === current.remote
        );
      });
      selected.value = selected.value.filter((id) => !statusFor(id)?.error);
      void refreshRepositories();
    }
    persisted = copy(project);
  },
  { deep: true },
);
watch(
  () => signature(draft.value),
  () => {
    plan.value = null;
    notice.value = '';
  },
);
watch(
  () => [draft.value.scmDevelopmentProjectId, draft.value.scmTestProjectId],
  () => {
    development.value = [];
    testing.value = [];
  },
);
watch(
  [selected, push],
  () => {
    plan.value = null;
  },
  { deep: true },
);
function connectionChanged(status: ScmStatus) {
  connected.value = status.connected;
  sessionExpired.value = false;
  development.value = [];
  testing.value = [];
  projects.value = [];
}
function loadProjects() {
  void perform('加载 SCM 项目', async (token) => {
    const result = await api.scmProjects();
    if (valid(token)) projects.value = result;
  });
}
function loadModules() {
  if (!connected.value) return;
  void perform('加载 SCM 模块', async (token) => {
    const devId = scmProjectId(draft.value.scmDevelopmentProjectId);
    const testId = testProjectId();
    const [dev, test] = await Promise.all([
      api.scmProjectModules(devId),
      testId ? api.scmProjectModules(testId) : Promise.resolve([]),
    ]);
    if (!valid(token)) return;
    development.value = dev;
    testing.value = test;
  });
}
function applyMapping() {
  if (locked.value || !development.value.length) return;
  try {
    const testId = testProjectId();
    const urls = Object.fromEntries(
      statuses.value
        .filter((status) => {
          const saved = props.project.subProjects.find((repo) => repo.id === status.id);
          const current = draft.value.subProjects.find((repo) => repo.id === status.id);
          // 状态中的 URL 对应已持久化 remote，不能用于猜测刚编辑的新 remote。
          return !!saved && saved.remote === current?.remote;
        })
        .map((status) => [status.id, status.remoteUrl]),
    );
    const eligible = draft.value.subProjects.filter((repo) => !statusFor(repo.id)?.error);
    if (!eligible.length) throw new Error('没有可匹配的 Git 仓库，请先检查子项目目录。');
    const mapped = mapBackendModules(
      eligible,
      urls,
      development.value,
      testId ? testing.value : undefined,
    );
    draft.value.subProjects = draft.value.subProjects.map(
      (repo) => mapped.find((item) => item.id === repo.id) ?? repo,
    );
    draft.value.scmDevelopmentProjectId = scmProjectId(draft.value.scmDevelopmentProjectId);
    draft.value.scmTestProjectId = testId ?? '';
    save();
  } catch (reason) {
    failure(reason);
  }
}
function prepare(operationType: 'switch' | 'merge') {
  if (!canPrepare.value) return;
  const repositories = copy(
    draft.value.subProjects.filter((item) => selected.value.includes(item.id)),
  );
  if (
    operationType === 'merge' &&
    repositories.some(
      (item) => !item.testBranch.trim() || item.testBranch === item.developmentBranch,
    )
  ) {
    error.value = '请为每个仓库配置明确且不同的开发分支和测试分支。';
    return;
  }
  plan.value = null;
  void perform('获取远端并预检', async (token) => {
    const result = await api.prepareBackendGit(
      props.project.id,
      repositories,
      operationType,
      operationType === 'merge' && push.value,
    );
    if (valid(token) && result.projectId === props.project.id) plan.value = result;
  });
}
function execute() {
  const confirmed = plan.value;
  if (!confirmed || dirty.value || locked.value) return;
  void perform('执行 Git 计划', async (token) => {
    const taskId = await api.executeBackendGit(confirmed.planId);
    if (!valid(token)) return;
    operation.value = {
      planId: confirmed.planId,
      taskId,
      status: 'running',
      repositories: confirmed.repositories.map((item) => ({
        repoId: item.repository.id,
        status: 'pending',
        message: '',
        worktreePath: '',
      })),
    };
    plan.value = null;
    emit('task', taskId, confirmed.operation === 'merge' ? '合并到测试分支' : '切换开发分支');
    await refreshState(confirmed.planId);
  });
}
async function cancel() {
  if (!desktop || !operation.value || activity.value) return;
  const token = generation;
  const current = operation.value;
  activity.value = '取消任务';
  try {
    await api.cancel(current.taskId);
    if (valid(token)) await refreshState(current.planId);
  } catch (reason) {
    if (valid(token)) failure(reason);
  } finally {
    if (valid(token)) activity.value = '';
  }
}
function resolveWorktree(repoId: string, action: 'abort' | 'complete') {
  const current = operation.value;
  if (!current || (action === 'complete' && !cleanConfirmed.value[`${current.planId}:${repoId}`]))
    return;
  void perform(action === 'abort' ? '中止合并' : '完成手工处理', async (token) => {
    if (action === 'abort') await api.abortBackendGit(current.planId, repoId);
    else await api.completeBackendGit(current.planId, repoId);
    if (valid(token)) await refreshState(current.planId);
  });
}
async function openPath(path: string) {
  if (!desktop) return;
  const token = generation;
  try {
    await api.openPath(path);
  } catch (reason) {
    if (valid(token)) failure(reason);
  }
}
onMounted(() => {
  if (!desktop) return;
  void refreshRepositories();
  void refreshHistory();
  void listen<{ planId: string }>('backend-git-updated', (event) => {
    if (!disposed && event.payload.planId === operation.value?.planId)
      void refreshState(event.payload.planId);
  })
    .then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    })
    .catch((reason) => {
      if (!disposed) failure(reason);
    });
});
onBeforeUnmount(() => {
  disposed = true;
  ++generation;
  ++stateSequence;
  unlisten?.();
});
</script>

<template>
  <section class="backend-workbench">
    <header class="workbench-heading">
      <div>
        <span class="eyebrow">REPOSITORIES &amp; DELIVERY</span>
        <h2>后端工作台</h2>
      </div>
      <button class="button small" :disabled="locked" @click="emit('edit')">
        编辑项目 / 扫描父目录
      </button>
    </header>
    <p v-if="!desktop" class="hint">浏览器预览：可编辑映射，SCM 与 Git 操作需要桌面应用。</p>
    <p v-if="error" role="alert" class="error">{{ error }}</p>
    <p v-if="waiting" role="status">
      等待保存确认…
      <button
        class="button small"
        v-if="!saving"
        @click="
          waiting = '';
          error = '尚未确认保存，可重试。';
        "
      >
        返回编辑
      </button>
    </p>
    <p v-else-if="notice" role="status">{{ notice }}</p>
    <p v-if="activity" role="status">{{ activity }}…</p>
    <ScmConnection
      :session-expired="sessionExpired"
      :disabled="locked"
      @change="connectionChanged"
    />
    <section class="workbench-card" aria-label="SCM 项目映射">
      <div class="section-heading">
        <h3>SCM 分支映射</h3>
        <button
          class="button small"
          :disabled="!desktop || !connected || locked"
          @click="loadProjects"
        >
          加载项目列表
        </button>
      </div>
      <fieldset :disabled="locked">
        <div class="scm-projects">
          <label
            >开发 SCM 项目 ID<input
              v-model="draft.scmDevelopmentProjectId"
              list="backend-scm-projects"
              placeholder="数字 ID 或完整项目链接" /></label
          ><label
            >测试 SCM 项目 ID（可选，仅合并时需要）<input
              v-model="draft.scmTestProjectId"
              list="backend-scm-projects"
              placeholder="数字 ID 或完整项目链接"
          /></label>
        </div>
      </fieldset>
      <datalist id="backend-scm-projects">
        <option v-for="project in projects" :key="project.id" :value="project.id">
          {{ project.name }}
        </option>
      </datalist>
      <div class="toolbar">
        <button
          class="button small"
          :disabled="!desktop || !connected || locked"
          @click="loadModules"
        >
          加载模块</button
        ><button
          class="button small"
          :disabled="locked || !development.length"
          @click="applyMapping"
        >
          应用映射并保存
        </button>
      </div>
      <div v-if="development.length || testing.length" class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>环境</th>
              <th>模块 / Git ID</th>
              <th>分支</th>
              <th>本地映射</th>
            </tr>
          </thead>
          <tbody>
            <template
              v-for="group in [
                { name: '开发', modules: development },
                { name: '测试', modules: testing },
              ]"
              :key="group.name"
              ><tr v-for="(module, index) in group.modules" :key="index">
                <td>{{ group.name }}</td>
                <td>
                  {{ module.moduleName }}<small>{{ module.gitId || '缺少 Git ID' }}</small>
                </td>
                <td>
                  <code>{{ module.branch }}</code>
                </td>
                <td>
                  {{
                    draft.subProjects
                      .filter((repo) => !!repo.scmGitId && repo.scmGitId === module.gitId)
                      .map((repo) => repo.name)
                      .join('、') || '尚未映射（按 Git ID / 仓库 URL 匹配）'
                  }}
                </td>
              </tr></template
            >
          </tbody>
        </table>
      </div>
    </section>
    <section class="workbench-card" aria-label="本地子仓库">
      <div class="section-heading">
        <h3>
          本地子仓库 <span>{{ draft.subProjects.length }}</span>
        </h3>
        <div class="toolbar">
          <button
            class="button small"
            :disabled="!desktop || locked"
            @click="refreshRepositories()"
          >
            刷新仓库</button
          ><button class="button small" data-save :disabled="locked || !dirty" @click="save">
            保存映射
          </button>
        </div>
      </div>
      <p v-if="!draft.subProjects.length" class="empty">
        尚未配置子项目，请通过“编辑项目 / 扫描父目录”添加。
      </p>
      <div v-for="repo in draft.subProjects" :key="repo.id" class="repository">
        <div class="repository-heading">
          <label class="repository-title"
            ><input
              v-model="selected"
              type="checkbox"
              :value="repo.id"
              :disabled="locked || !!statusFor(repo.id)?.error"
            /><strong>{{ repo.name }}</strong></label
          >
          <div class="toolbar">
            <button class="button small" :disabled="!desktop" @click="openPath(repo.path)">
              打开目录</button
            ><button class="button small" :disabled="locked" @click="emit('edit')">
              编辑子项目
            </button>
          </div>
        </div>
        <p class="repository-path">{{ repo.path }}</p>
        <div class="repository-state">
          <code>{{ statusFor(repo.id)?.currentBranch || '分支待刷新' }}</code
          ><span v-if="statusFor(repo.id)?.dirty" class="warning">有未提交更改</span
          ><span v-if="statusFor(repo.id)?.merging" class="warning">合并中</span
          ><span v-if="statusFor(repo.id)?.rebasing" class="warning">变基中</span
          ><span v-if="statusFor(repo.id)?.error" class="error">{{
            statusFor(repo.id)?.error
          }}</span>
        </div>
        <fieldset :disabled="locked">
          <div class="mapping-fields">
            <label>Remote<input v-model="repo.remote" /></label
            ><label
              >开发分支<input
                v-model="repo.developmentBranch"
                data-development
                :list="`branches-${repo.id}`" /></label
            ><label>测试分支<input v-model="repo.testBranch" :list="`branches-${repo.id}`" /></label
            ><label>SCM Git ID<input v-model="repo.scmGitId" :list="`modules-${repo.id}`" /></label>
          </div>
        </fieldset>
        <datalist :id="`branches-${repo.id}`">
          <option
            v-for="branch in statusFor(repo.id)?.branches || []"
            :key="branch"
            :value="branch"
          /></datalist
        ><datalist :id="`modules-${repo.id}`">
          <option v-for="(module, index) in development" :key="index" :value="module.gitId || ''">
            {{ module.moduleName }} · {{ module.branch }}
          </option>
        </datalist>
      </div>
    </section>
    <section class="workbench-card" aria-label="Git 批量操作">
      <div class="section-heading">
        <h3>Git 批量操作</h3>
        <span class="hint">已选 {{ selected.length }} 个仓库</span>
      </div>
      <p class="hint">
        先获取远端并预检，核对计划后执行。{{ dirty ? '映射有未保存更改，请先保存。' : '' }}
      </p>
      <div class="toolbar">
        <button class="button small" :disabled="!canPrepare" @click="prepare('switch')">
          切换开发分支</button
        ><button class="button small" :disabled="!canPrepare" @click="prepare('merge')">
          合并到测试分支</button
        ><label class="checkbox"
          ><input v-model="push" type="checkbox" :disabled="locked" />合并成功后推送测试分支</label
        >
      </div>
      <div v-if="plan" class="plan">
        <h4>确认 Git 计划</h4>
        <p>
          {{ plan.operation === 'merge' ? '合并' : '切换分支' }} ·
          {{ plan.push ? '将推送测试分支' : '不推送远端' }}
        </p>
        <ul>
          <li v-for="item in plan.repositories" :key="item.repository.id">
            <strong>{{ item.repository.name }}</strong>
            <code>{{ item.currentBranch }} → {{ item.targetBranch }}</code
            ><small>来源提交：{{ item.sourceSha }}</small>
            <small>目标提交：{{ item.targetSha }}</small>
            <span
              v-if="
                plan.operation === 'merge' && item.remoteSha && item.targetSha !== item.remoteSha
              "
              class="warning"
            >
              测试分支已有本地未推送提交，本次操作会保留；勾选推送时将一并推送。
            </span>
          </li>
        </ul>
        <div class="toolbar">
          <button class="button small primary" :disabled="locked || dirty" @click="execute">
            确认执行计划</button
          ><button class="button small" :disabled="locked" @click="plan = null">取消计划</button>
        </div>
      </div>
      <div class="toolbar history-toolbar">
        <button class="button small" :disabled="!desktop || locked" @click="refreshHistory()">
          读取历史操作</button
        ><label v-if="history.length"
          >操作记录<select
            :value="operation?.planId || ''"
            :disabled="locked"
            @change="
              operation =
                history.find(
                  (item) => item.planId === ($event.target as HTMLSelectElement).value,
                ) || null;
              plan = null;
            "
          >
            <option value="">选择操作</option>
            <option v-for="item in history" :key="item.planId" :value="item.planId">
              {{ item.planId }} · {{ item.status }}
            </option>
          </select></label
        >
      </div>
      <div v-if="operation" class="operation">
        <div class="section-heading">
          <h4>操作状态 · {{ operation.status }}</h4>
          <button
            class="button small"
            v-if="operation.status === 'running'"
            :disabled="!!activity"
            @click="cancel"
          >
            取消任务
          </button>
        </div>
        <p v-if="operation.status === 'paused'" class="warning">
          遇错暂停；尚未执行的仓库保持 pending。请先处理保留的工作树。
        </p>
        <div v-for="item in operation.repositories" :key="item.repoId" class="result">
          <strong>{{
            draft.subProjects.find((repo) => repo.id === item.repoId)?.name || item.repoId
          }}</strong
          ><span :class="item.status === 'error' ? 'error' : ''">{{ item.status }}</span>
          <p>{{ item.message }}</p>
          <template v-if="item.worktreePath"
            ><code>{{ item.worktreePath }}</code>
            <label class="checkbox"
              ><input
                v-model="cleanConfirmed[`${operation.planId}:${item.repoId}`]"
                type="checkbox"
                :disabled="locked"
              />已手工提交并确认工作树干净</label
            >
            <div class="toolbar">
              <button class="button small" @click="openPath(item.worktreePath)">打开工作树</button
              ><button
                class="button small"
                :disabled="locked || !cleanConfirmed[`${operation.planId}:${item.repoId}`]"
                @click="resolveWorktree(item.repoId, 'complete')"
              >
                已手工解决，完成处理</button
              ><button
                class="button small"
                :disabled="locked"
                @click="resolveWorktree(item.repoId, 'abort')"
              >
                中止该次合并
              </button>
            </div>
            <small
              >完成前请手工解决冲突、提交并确认工作树干净；完成处理不会自动推送。</small
            ></template
          >
        </div>
      </div>
    </section>
  </section>
</template>

<style scoped>
.backend-workbench {
  display: grid;
  gap: 14px;
  color: var(--ink);
}
.workbench-heading,
.section-heading,
.repository-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
h2 {
  margin: 5px 0 0;
  font-size: 22px;
}
h3,
h4 {
  margin: 0;
  font-size: 14px;
}
h3 span {
  margin-left: 6px;
  color: var(--muted);
  font-size: 12px;
}
.eyebrow {
  color: var(--accent-text);
  font-size: 10px;
  letter-spacing: 0.14em;
  font-weight: 700;
}
.workbench-card {
  background: var(--paper);
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 16px;
  min-width: 0;
}
.section-heading {
  margin-bottom: 12px;
}
.toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar button {
  font-size: 12px;
}
fieldset {
  padding: 0;
  border: 0;
  margin: 0 0 10px;
  min-width: 0;
}
label {
  display: grid;
  gap: 5px;
  color: var(--text-secondary);
  font-size: 12px;
}
input:not([type='checkbox']) {
  width: 100%;
  min-width: 0;
}
.scm-projects {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}
.mapping-fields {
  display: grid;
  grid-template-columns: 0.7fr 1fr 1fr 1fr;
  gap: 10px;
}
.repository {
  border-top: 1px solid var(--line);
  padding-top: 12px;
  margin-top: 12px;
}
.repository-title {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  color: var(--ink);
}
.repository-path {
  color: var(--muted);
  font-family: var(--mono);
  font-size: 11px;
  margin: 4px 0 8px 23px;
  overflow-wrap: anywhere;
}
.repository-state {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  margin: 0 0 12px 23px;
}
.repository-state code {
  color: var(--accent-text);
  background: var(--accent-soft);
  padding: 3px 7px;
  border-radius: 4px;
}
.hint,
small,
.empty {
  color: var(--muted);
  font-size: 12px;
}
.error {
  color: var(--danger-text);
}
.warning {
  color: var(--warning-text);
}
.checkbox {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 5px;
}
.table-wrap {
  overflow: auto;
  margin-top: 12px;
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}
th,
td {
  text-align: left;
  padding: 9px 8px;
  border-bottom: 1px solid var(--line);
}
th {
  color: var(--muted);
  font-weight: 500;
}
small {
  display: block;
  margin-top: 5px;
}
code {
  font-family: var(--mono);
  font-size: 11px;
  overflow-wrap: anywhere;
}
.plan {
  background: var(--accent-soft);
  border: 1px solid var(--accent-border);
  padding: 14px;
  border-radius: 7px;
  margin-top: 14px;
}
.plan p {
  font-size: 12px;
}
.plan ul {
  padding-left: 18px;
}
.plan li {
  margin-bottom: 10px;
}
.plan li code {
  margin-left: 8px;
}
.history-toolbar {
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--line);
}
.history-toolbar label {
  min-width: 150px;
}
.operation {
  margin-top: 16px;
}
.result {
  border-top: 1px solid var(--line);
  padding: 12px 0;
  font-size: 12px;
}
.result > span {
  margin-left: 12px;
}
.result .toolbar {
  margin-top: 8px;
}
@media (max-width: 760px) {
  .mapping-fields,
  .scm-projects {
    grid-template-columns: 1fr 1fr;
  }
  .workbench-heading {
    align-items: flex-start;
  }
  .section-heading {
    flex-wrap: wrap;
  }
}
@media (max-width: 480px) {
  .mapping-fields,
  .scm-projects {
    grid-template-columns: 1fr;
  }
}
</style>
