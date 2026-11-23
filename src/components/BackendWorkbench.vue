<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api, desktop } from '../api';
import { hasMatchableModuleUrl, syncBackendModules, scmProjectId } from '../backend-mapping';
import type {
  Project,
  BackendGitPlan,
  BackendGitOperation,
  PomBlockChoice,
  BackendRepositoryStatus,
  ScmModule,
  ScmProject,
  ScmStatus,
} from '../types';
import ScmConnection from './ScmConnection.vue';
import ScmProjectInput from './ScmProjectInput.vue';
const props = defineProps<{ project: Project; saving: boolean; busy: boolean }>();
const emit = defineEmits<{
  save: [project: Project];
  edit: [];
  task: [taskId: string, title: string];
  notice: [message: string, error?: boolean];
}>();
const copy = <T,>(value: T): T => JSON.parse(JSON.stringify(value));
const draft = ref<Project>(copy(props.project));
const statuses = ref<BackendRepositoryStatus[]>([]);
const repositoryLoading = ref(false);
const selected = ref<string[]>(props.project.subProjects.map((item) => item.id));
const error = ref('');
const notice = ref('');
const activity = ref('');
const waiting = ref('');
const plan = ref<(BackendGitPlan & { continuedFrom?: string }) | null>(null);
const planHeading = ref<HTMLElement | null>(null);
const operation = ref<BackendGitOperation | null>(null);
const canContinue = computed(() => {
  const current = operation.value;
  return (
    current?.status === 'paused' &&
    current.repositories.some((repo) => repo.status === 'pending') &&
    current.repositories.every((repo) => !repo.worktreePath)
  );
});
const history = ref<BackendGitOperation[]>([]);
const confirmingClearHistory = ref(false);
const clearingHistory = ref(false);
// 执行结果可能已结束，不应当作为待处理选项显示。
const selectedHistoryId = computed({
  get: () => history.value.find((item) => item.planId === operation.value?.planId)?.planId ?? '',
  set: (id: string) => {
    const selectedOperation = history.value.find((item) => item.planId === id);
    if (!selectedOperation) return;
    operation.value = selectedOperation;
    plan.value = null;
  },
});
const operationLabels: Record<BackendGitOperation['status'], string> = {
  prepared: '等待执行',
  running: '执行中',
  success: '已完成',
  paused: '已暂停，需处理',
  cancelled: '已取消',
  aborting: '正在中止',
};
function repositoryName(id: string): string {
  return draft.value.subProjects.find((repo) => repo.id === id)?.name || '已移除的仓库';
}
function operationSummary(item: BackendGitOperation): string {
  const success = item.repositories.filter((repo) => repo.status === 'success').length;
  const failed = item.repositories.filter((repo) => repo.status === 'error').length;
  const pending = item.repositories.filter((repo) => repo.status === 'pending').length;
  return `共 ${item.repositories.length} 个仓库 · 成功 ${success} · 失败 ${failed} · 未执行 ${pending}`;
}
function historyLabel(item: BackendGitOperation): string {
  return `${item.repositories.map((repo) => repositoryName(repo.repoId)).join('、')} · ${operationLabels[item.status]}`;
}
function repositoryStatus(item: BackendGitOperation['repositories'][number]): string {
  return {
    pending: ['paused', 'cancelled'].includes(operation.value?.status || '')
      ? '未执行'
      : '等待执行',
    running: '执行中',
    success: '成功',
    error: '失败',
    cancelled: '已取消',
    aborted: '已中止',
  }[item.status];
}
function repositoryMessage(item: BackendGitOperation['repositories'][number]): string {
  if (item.message) return item.message;
  if (item.status === 'pending') {
    if (operation.value?.status === 'paused')
      return '批次已暂停，该仓库尚未执行；请重新预检后操作。';
    if (operation.value?.status === 'cancelled') return '任务已取消，该仓库尚未执行。';
    return '正在等待执行，尚未修改该仓库。';
  }
  return {
    running: '正在执行 Git 操作，请稍候。',
    success: '操作已完成。',
    error: '执行失败，请检查仓库状态后重新预检。',
    cancelled: '任务已取消，请刷新仓库确认当前状态。',
    aborted: '本次合并已中止。',
  }[item.status];
}
// 预检也会持久化计划，但未启动任务的记录不属于需要恢复的实际操作。
function needsRecovery(item: BackendGitOperation): boolean {
  return (
    item.repositories.some((repo) => !!repo.worktreePath) ||
    (item.status !== 'success' &&
      item.status !== 'cancelled' &&
      (item.status === 'running' ||
        item.status === 'aborting' ||
        !!item.taskId ||
        item.repositories.some((repo) => repo.status !== 'pending')))
  );
}
const push = ref(false);
const pomChoices = ref<Record<string, PomBlockChoice['side']>>({});
const cleanConfirmed = ref<Record<string, boolean>>({});
const connected = ref(false);
const sessionExpired = ref(false);
const projects = ref<ScmProject[]>([]);
const development = ref<ScmModule[]>([]);
const testing = ref<ScmModule[]>([]);
const loadedDevelopmentId = ref('');
const loadedTestId = ref<string | undefined>();
interface LoadedModules {
  devId: string;
  testId?: string;
  dev: ScmModule[];
  test: ScmModule[];
}
const MODULE_LOAD_LABEL = '加载 SCM 模块';
const mappingRemoteUrls = ref<Record<string, string>>({});
const readIdentities = ref<Record<string, string>>({});
const repositoryIdentity = (repo: Project['subProjects'][number]) =>
  JSON.stringify([repo.path, repo.remote, repo.scmGitId]);
function sourceId(value: string): string {
  try {
    return value.trim() ? scmProjectId(value) : '';
  } catch {
    return value.trim();
  }
}
let generation = 0;
let stateSequence = 0;
let repositorySequence = 0;
let historySequence = 0;
let historyProjectGeneration = 0;
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
  () =>
    props.saving ||
    props.busy ||
    !!activity.value ||
    clearingHistory.value ||
    repositoryLoading.value ||
    running.value ||
    !!waiting.value,
);
const statusFor = (id: string) => {
  const repo = draft.value.subProjects.find((item) => item.id === id);
  return repo && readIdentities.value[id] === repositoryIdentity(repo)
    ? statuses.value.find((item) => item.id === id)
    : undefined;
};
const developmentLoaded = computed(
  () =>
    !!loadedDevelopmentId.value &&
    loadedDevelopmentId.value === sourceId(draft.value.scmDevelopmentProjectId),
);
// 以开发模块为展示范围，按仓库编号配对测试分支，不按名称猜测。
const moduleRows = computed(() =>
  development.value.map((module) => {
    const gitId = module.gitId?.trim();
    return {
      module,
      testing: gitId ? testing.value.filter((candidate) => candidate.gitId?.trim() === gitId) : [],
      repositories: gitId
        ? visibleRepositories.value.filter((repo) => repo.scmGitId.trim() === gitId)
        : [],
    };
  }),
);
const mapping = computed(() => {
  const result = syncBackendModules(
    draft.value.subProjects.filter((repo) => !statusFor(repo.id)?.error),
    Object.fromEntries(
      draft.value.subProjects.map((repo) => [
        repo.id,
        readIdentities.value[repo.id] === repositoryIdentity(repo)
          ? (mappingRemoteUrls.value[repo.id] ?? '')
          : '',
      ]),
    ),
    developmentLoaded.value ? development.value : [],
    loadedTestId.value ? testing.value : undefined,
  );
  return {
    ...result,
    repositories: draft.value.subProjects.map(
      (repo) => result.repositories.find((item) => item.id === repo.id) ?? repo,
    ),
  };
});
const visibleRepositories = computed(() =>
  developmentLoaded.value
    ? draft.value.subProjects.filter((repo) => mapping.value.visibleRepositoryIds.includes(repo.id))
    : [],
);
const selectedRepositories = computed(() =>
  visibleRepositories.value.filter((repo) => selected.value.includes(repo.id)),
);
function setVisibleRepositoriesSelected(checked: boolean) {
  // 保留当前 SCM 列表之外的勾选状态。
  const visibleIds = new Set(visibleRepositories.value.map((repo) => repo.id));
  const otherIds = selected.value.filter((id) => !visibleIds.has(id));
  selected.value = checked ? [...otherIds, ...visibleIds] : otherIds;
}
function localBranch(id: string): string {
  const status = statusFor(id);
  if (!status) return '分支待刷新';
  if (status.error) return '读取失败';
  return (
    status.currentBranch || (status.head ? `游离 HEAD (${status.head.slice(0, 8)})` : '未创建分支')
  );
}
async function copyBranch(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    if (!disposed) emit('notice', '分支名已复制');
  } catch {
    if (!disposed) emit('notice', '复制失败，请手动选择分支名复制。', true);
  }
}
// 仅真实分支提供鼠标和键盘复制，状态占位文字保持普通文本。
function branchCopyProps(value?: string) {
  if (!value?.trim()) return {};
  return {
    class: 'branch-copyable',
    role: 'button',
    tabindex: 0,
    title: `点击复制分支名：${value}`,
    'aria-label': `复制分支 ${value}`,
    onClick: () => void copyBranch(value),
    onKeydown: (event: KeyboardEvent) => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        void copyBranch(value);
      }
    },
  };
}
const gitActionLabels = {
  switch: '切换开发分支',
  'switch-test': '切换到测试分支',
  'switch-master': '切换master分支',
  'push-dev': '推送开发分支到远程',
  merge: '合并到测试分支',
} satisfies Record<BackendGitPlan['operation'], string>;
function disabledReason(operationType: BackendGitPlan['operation']): string {
  if (!desktop) return '请在桌面应用中操作。';
  if (locked.value)
    return waiting.value ? '等待保存确认后可操作。' : '加载、保存或执行中，请稍候。';
  if (!developmentLoaded.value) return '请先加载当前开发 SCM 项目并匹配本地仓库。';
  if (!visibleRepositories.value.length) return '没有匹配的本地仓库，请添加路径或核对 SCM Git ID。';
  if (!selectedRepositories.value.length) return '请至少勾选一个关联仓库。';
  if (dirty.value) return 'SCM 配置有更改，请重新加载模块并等待自动保存。';
  if (
    selectedRepositories.value.some((repo) => !repo.remote.trim() || !repo.developmentBranch.trim())
  )
    return '请补全所选仓库的 remote 和开发分支。';
  if (
    selectedRepositories.value.some((repo) => {
      const status = statusFor(repo.id);
      return !status || !!status.error || status.dirty || status.merging || status.rebasing;
    })
  )
    return '所选仓库状态未就绪，请刷新并处理未提交更改、合并或变基。';
  if (
    operationType === 'push-dev' &&
    selectedRepositories.value.some(
      (repo) => statusFor(repo.id)?.currentBranch !== repo.developmentBranch,
    )
  )
    return '请先切换开发分支，再推送开发分支到远程。';
  if (operationType === 'switch-test' || operationType === 'merge') {
    if (!draft.value.scmTestProjectId.trim()) return '请填写测试 SCM 项目 ID 并加载测试目标。';
    if (loadedTestId.value !== sourceId(draft.value.scmTestProjectId))
      return '测试 SCM 项目已变更，请先加载测试目标。';
    if (operationType === 'switch-test') {
      if (selectedRepositories.value.some((repo) => !repo.testBranch.trim()))
        return '请为每个仓库配置明确的测试分支。';
    } else if (
      selectedRepositories.value.some(
        (repo) =>
          !repo.testBranch.trim() || repo.testBranch.trim() === repo.developmentBranch.trim(),
      )
    )
      return '请为每个仓库配置明确且不同的开发分支和测试分支。';
  }
  return '';
}
const switchDisabledReason = computed(() => disabledReason('switch'));
const switchTestDisabledReason = computed(() => disabledReason('switch-test'));
const switchMasterDisabledReason = computed(() => disabledReason('switch-master'));
const pushDevDisabledReason = computed(() => disabledReason('push-dev'));
const mergeDisabledReason = computed(() => disabledReason('merge'));
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
function invalidateRepositoryRefresh() {
  ++repositorySequence;
  repositoryLoading.value = false;
}
async function refreshRepositories(
  token = generation,
  fillMapping = false,
  modules?: LoadedModules,
): Promise<boolean> {
  if (!desktop || !valid(token)) return false;
  if (!modules && (!developmentLoaded.value || activity.value === MODULE_LOAD_LABEL)) return false;
  const sequence = ++repositorySequence;
  const current = () => valid(token) && sequence === repositorySequence;
  repositoryLoading.value = true;
  const repositories = copy(draft.value.subProjects);
  const dev = modules?.dev ?? development.value;
  const fillIds = fillMapping
    ? repositories
        .filter((repo) => readIdentities.value[repo.id] !== repositoryIdentity(repo))
        .map((repo) => repo.id)
    : [];
  let synchronized = false;
  try {
    // 仅重新加载或仓库身份改变时读取 URL，普通刷新复用已加载的匹配范围。
    const candidates = hasMatchableModuleUrl(dev)
      ? repositories.filter((repo) => !repo.scmGitId.trim())
      : [];
    const unread = candidates.filter(
      (repo) => modules || readIdentities.value[repo.id] !== repositoryIdentity(repo),
    );
    const unreadIds = new Set(unread.map((repo) => repo.id));
    const cachedUrls = Object.fromEntries(
      candidates
        .filter((repo) => !unreadIds.has(repo.id))
        .map((repo) => [repo.id, mappingRemoteUrls.value[repo.id] ?? '']),
    );
    const remoteUrls = unread.length
      ? { ...cachedUrls, ...(await api.backendRepositoryRemoteUrls(unread)) }
      : cachedUrls;
    if (!current()) return false;
    const { visibleRepositoryIds } = syncBackendModules(repositories, remoteUrls, dev);
    const matchedIds = new Set(visibleRepositoryIds);
    const matched = repositories.filter((repo) => matchedIds.has(repo.id));
    const result = matched.length ? await api.backendRepositoryStatuses(matched) : [];
    if (!current()) return false;
    // 若两次读取之间远端地址有变化，以完整状态中的最新地址为准。
    for (const status of result) {
      if (!status.error && Object.hasOwn(remoteUrls, status.id))
        remoteUrls[status.id] = status.remoteUrl;
    }
    statuses.value = result;
    mappingRemoteUrls.value = remoteUrls;
    readIdentities.value = Object.fromEntries(
      repositories.map((repo) => [repo.id, repositoryIdentity(repo)]),
    );
    const unavailable = new Set(result.filter((item) => item.error).map((item) => item.id));
    selected.value = selected.value.filter((id) => !unavailable.has(id));
    if (modules) {
      development.value = modules.dev;
      testing.value = modules.test;
      loadedDevelopmentId.value = modules.devId;
      loadedTestId.value = modules.testId;
      applyLoadedMapping();
    } else if (fillIds.length) {
      // 普通刷新只更新状态，外部新增或身份改变后同步分支并保存。
      applyLoadedMapping(fillIds);
      synchronized = true;
    }
    return true;
  } catch (reason) {
    if (current()) failure(reason);
  } finally {
    if (sequence === repositorySequence) {
      repositoryLoading.value = false;
      if (synchronized && valid(token)) saveLoadedMapping();
    }
  }
  return false;
}
function currentHistoryProject(token: number) {
  return !disposed && token === historyProjectGeneration;
}
async function refreshHistory(autoSelect = true) {
  if (!desktop) return;
  const token = historyProjectGeneration;
  const sequence = ++historySequence;
  try {
    const result = await api.backendGitOperations(props.project.id);
    if (!currentHistoryProject(token) || sequence !== historySequence) return;
    history.value = result.filter(needsRecovery);
    if (autoSelect && !operation.value)
      operation.value =
        history.value.find((item) => ['running', 'aborting'].includes(item.status)) ??
        history.value[0] ??
        null;
  } catch (reason) {
    if (currentHistoryProject(token) && sequence === historySequence) failure(reason);
  }
}
async function clearHistory() {
  if (!desktop || locked.value || !confirmingClearHistory.value) return;
  const token = historyProjectGeneration;
  const selectedPlanId = selectedHistoryId.value || undefined;
  const selectedHasWorktree =
    !!selectedPlanId && operation.value?.repositories.some((repo) => !!repo.worktreePath);
  clearingHistory.value = true;
  error.value = '';
  try {
    const removed = await api.backendClearGitOperations(props.project.id, selectedPlanId);
    if (!currentHistoryProject(token)) return;
    // 以实际删除的计划为准，不能把仍在运行或可恢复的结果一并重置。
    const removedIds = new Set(removed);
    history.value = history.value.filter((item) => !removedIds.has(item.planId));
    if (operation.value && removedIds.has(operation.value.planId)) operation.value = null;
    if (plan.value?.continuedFrom && removedIds.has(plan.value.continuedFrom)) plan.value = null;
    confirmingClearHistory.value = false;
    const summary = removed.length
      ? `已清除 ${removed.length} 条 Git 操作记录`
      : '没有可清除的 Git 操作记录';
    const retainedNotice = selectedHasWorktree
      ? '选中的操作仍有保留工作区，已保留该记录。'
      : '选中的操作仍需保留，未清除。';
    emit(
      'notice',
      selectedPlanId && !removedIds.has(selectedPlanId) ? `${summary}。${retainedNotice}` : summary,
    );
    await refreshHistory(false);
  } catch (reason) {
    if (currentHistoryProject(token))
      error.value = `清除 Git 操作记录失败：${String(reason instanceof Error ? reason.message : reason)}`;
  } finally {
    if (currentHistoryProject(token)) clearingHistory.value = false;
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
    history.value = [...history.value.filter((item) => item.planId !== planId), state].filter(
      needsRecovery,
    );
    if (!['running', 'aborting'].includes(state.status))
      void refreshRepositories(token, repositoryLoading.value);
  } catch (reason) {
    if (valid(token) && sequence === stateSequence) failure(reason);
  }
}
function saveLoadedMapping() {
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
      error.value = '自动保存未完成，请检查提示后重新加载模块重试。';
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
      ++historyProjectGeneration;
      invalidateRepositoryRefresh();
      confirmingClearHistory.value = false;
      clearingHistory.value = false;
      draft.value = copy(project);
      selected.value = project.subProjects.map((item) => item.id);
      statuses.value = [];
      plan.value = null;
      operation.value = null;
      history.value = [];
      development.value = [];
      testing.value = [];
      loadedDevelopmentId.value = '';
      loadedTestId.value = undefined;
      readIdentities.value = {};
      mappingRemoteUrls.value = {};
      push.value = false;
      cleanConfirmed.value = {};
      pomChoices.value = {};
      waiting.value = '';
      notice.value = '';
      error.value = '';
      activity.value = '';
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
      // 状态按草稿身份校验；保存回执中的新 remote 已读取过，不能按旧 props 丢弃。
      const currentIds = new Set(project.subProjects.map((repo) => repo.id));
      statuses.value = statuses.value.filter((status) => currentIds.has(status.id));
      selected.value = selected.value.filter((id) => !statusFor(id)?.error);
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
  () => sourceId(draft.value.scmDevelopmentProjectId),
  () => {
    ++generation;
    invalidateRepositoryRefresh();
    activity.value = '';
    loadedDevelopmentId.value = '';
    loadedTestId.value = undefined;
    development.value = [];
    testing.value = [];
  },
  { flush: 'sync' },
);
watch(
  () => sourceId(draft.value.scmTestProjectId),
  () => {
    ++generation;
    invalidateRepositoryRefresh();
    activity.value = '';
    loadedTestId.value = undefined;
    testing.value = [];
  },
  { flush: 'sync' },
);
watch(
  () => draft.value.subProjects.map((repo) => [repo.id, repositoryIdentity(repo)] as const),
  (current, previous) => {
    const old = new Map(previous);
    const changedIds = current
      .filter(([id, identity]) => old.get(id) !== identity && readIdentities.value[id] !== identity)
      .map(([id]) => id);
    if (repositoryLoading.value || changedIds.length || current.length !== previous.length)
      void refreshRepositories(generation, true);
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
  ++generation;
  invalidateRepositoryRefresh();
  activity.value = '';
  connected.value = status.connected;
  sessionExpired.value = false;
  development.value = [];
  testing.value = [];
  loadedDevelopmentId.value = '';
  loadedTestId.value = undefined;
  projects.value = [];
}
function loadProjects() {
  void perform('加载 SCM 项目', async (token) => {
    const result = await api.scmProjects();
    if (valid(token)) projects.value = result;
  });
}
function applyLoadedMapping(ids?: string[]) {
  const mapped = mapping.value.repositories;
  draft.value.subProjects = draft.value.subProjects.map((repo) => {
    const updated =
      !ids || ids.includes(repo.id) ? (mapped.find((item) => item.id === repo.id) ?? repo) : repo;
    return draft.value.scmTestProjectId.trim() ? updated : { ...updated, testBranch: '' };
  });
  // 映射仅修改 SCM 身份和分支；刚读取的 remote 状态仍属于同一仓库。
  readIdentities.value = Object.fromEntries(
    draft.value.subProjects.map((repo) => [repo.id, repositoryIdentity(repo)]),
  );
}
async function loadModules() {
  if (!connected.value || !desktop || locked.value) return;
  let loaded = false;
  const token = generation;
  await perform(MODULE_LOAD_LABEL, async (currentToken) => {
    const devId = scmProjectId(draft.value.scmDevelopmentProjectId);
    const testId = testProjectId();
    const [dev, test] = await Promise.all([
      api.scmProjectModules(devId),
      testId ? api.scmProjectModules(testId) : Promise.resolve([]),
    ]);
    loaded = await refreshRepositories(currentToken, false, { devId, testId, dev, test });
  });
  // 保存必须在 perform 释放 activity 锁后触发，父配置回执才表示保存成功。
  if (loaded && valid(token)) saveLoadedMapping();
}
function prepare(operationType: BackendGitPlan['operation']) {
  if (disabledReason(operationType)) return;
  const repositories = copy(selectedRepositories.value);
  plan.value = null;
  void perform('获取远端并预检', async (token) => {
    const result = await api.prepareBackendGit(
      props.project.id,
      repositories,
      operationType,
      operationType === 'push-dev' || (operationType === 'merge' && push.value),
      false,
    );
    if (valid(token) && result.projectId === props.project.id) plan.value = result;
  });
}
function prepareRemaining() {
  const current = operation.value;
  if (!canContinue.value || !current || dirty.value || locked.value) return;
  plan.value = null;
  void perform('预检未执行的仓库', async (token) => {
    const result = await api.prepareRemainingBackendGit(current.planId);
    if (!valid(token) || result.projectId !== props.project.id) return;
    plan.value = { ...result, continuedFrom: current.planId };
    await nextTick();
    if (valid(token)) planHeading.value?.focus();
  });
}
function execute() {
  const confirmed = plan.value;
  if (!confirmed || dirty.value || locked.value) return;
  void perform('执行 Git 计划', async (token) => {
    const taskId = await api.executeBackendGit(confirmed.planId);
    if (!valid(token)) return;
    if (confirmed.continuedFrom) {
      ++historySequence;
      history.value = history.value.filter((item) => item.planId !== confirmed.continuedFrom);
    }
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
    emit('task', taskId, gitActionLabels[confirmed.operation]);
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
function resolvePomConflict(repoId: string, side: 'test' | 'development') {
  const current = operation.value;
  if (!current || current.status !== 'paused') return;
  void perform('采用选中的 POM 版本', async (token) => {
    await api.resolveBackendPomConflict(current.planId, repoId, side);
    if (valid(token)) await refreshState(current.planId);
  });
}
type GitRepositoryResult = BackendGitOperation['repositories'][number];
function pomChoiceKey(repoId: string, path: string, line: number) {
  return encodeURIComponent(JSON.stringify([operation.value?.planId, repoId, path, line]));
}
function selectedPomChoices(item: GitRepositoryResult): PomBlockChoice[] | null {
  const choices: PomBlockChoice[] = [];
  for (const file of item.pomConflictFiles ?? []) {
    for (const { line } of file.blocks) {
      const side = pomChoices.value[pomChoiceKey(item.repoId, file.path, line)];
      if (!side) return null;
      choices.push({ path: file.path, line, side });
    }
  }
  return choices.length ? choices : null;
}
function applyPomChoices(item: GitRepositoryResult) {
  const current = operation.value;
  const choices = selectedPomChoices(item);
  if (current?.status !== 'paused' || !item.pomBlocksResolution || !choices) return;
  void perform('应用 POM 冲突选择', async (token) => {
    await api.resolveBackendPomBlocks(current.planId, item.repoId, choices);
    if (valid(token)) {
      for (const { path, line } of choices)
        delete pomChoices.value[pomChoiceKey(item.repoId, path, line)];
      await refreshState(current.planId);
    }
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
function openWorktreeInIde(repoId: string, ide: 'IDEA' | 'WebStorm') {
  const current = operation.value;
  if (!current) return;
  void perform(`在 ${ide} 中打开工作树`, async () => {
    const open = ide === 'IDEA' ? api.openBackendWorktreeInIdea : api.openBackendWorktreeInWebStorm;
    await open(current.planId, repoId);
  });
}
onMounted(() => {
  if (!desktop) return;
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
    <section class="workbench-card" aria-label="SCM 项目与仓库">
      <div class="section-heading">
        <h3>
          SCM 项目与仓库
          <span>关联 {{ visibleRepositories.length }} / 配置 {{ draft.subProjects.length }}</span>
        </h3>
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
          <ScmProjectInput
            id="scm-development-project"
            v-model="draft.scmDevelopmentProjectId"
            label="开发 SCM 项目 ID"
            :projects="projects"
            :disabled="locked"
          />
          <ScmProjectInput
            id="scm-test-project"
            v-model="draft.scmTestProjectId"
            label="测试 SCM 项目 ID（可选）"
            :projects="projects"
            :disabled="locked"
          />
        </div>
      </fieldset>
      <div class="toolbar">
        <button
          class="button small"
          :disabled="!desktop || !connected || locked"
          @click="loadModules"
        >
          加载模块
        </button>
        <button
          class="button small"
          :disabled="!desktop || !developmentLoaded || locked"
          :title="!developmentLoaded ? '请先加载开发 SCM 模块。' : undefined"
          @click="refreshRepositories()"
        >
          刷新仓库
        </button>
        <button
          type="button"
          class="button small"
          :disabled="locked || selectedRepositories.length === visibleRepositories.length"
          @click="setVisibleRepositoriesSelected(true)"
        >
          全选
        </button>
        <button
          type="button"
          class="button small"
          :disabled="locked || !selectedRepositories.length"
          @click="setVisibleRepositoriesSelected(false)"
        >
          取消全选
        </button>
      </div>
      <p
        v-for="warning in developmentLoaded ? mapping.warnings : []"
        :key="warning"
        class="warning"
      >
        {{ warning }}
      </p>
      <p v-if="!draft.subProjects.length" class="empty">
        尚未配置子项目，请通过“编辑项目 / 扫描父目录”添加。
      </p>
      <p v-else-if="developmentLoaded && !visibleRepositories.length" class="empty">
        暂无关联仓库，请编辑项目添加本地路径或核对 SCM Git ID。
      </p>
      <div v-for="repo in visibleRepositories" :key="repo.id" class="repository">
        <div class="repository-heading">
          <div class="repository-identity">
            <label class="repository-title"
              ><input
                v-model="selected"
                type="checkbox"
                :value="repo.id"
                :disabled="locked || !!statusFor(repo.id)?.error"
              /><strong>{{ repo.name }}</strong></label
            >
            <span
              v-if="repo.scmModuleName && repo.scmModuleName !== repo.name"
              class="repository-metadata"
              >模块：{{ repo.scmModuleName }}</span
            >
            <span class="repository-metadata"
              >Remote：<code data-remote>{{ repo.remote || '未配置' }}</code></span
            >
            <span class="repository-metadata"
              >SCM Git ID：<code data-scm-git-id>{{ repo.scmGitId || '未关联' }}</code></span
            >
          </div>
          <div class="toolbar">
            <button class="button small" :disabled="!desktop" @click="openPath(repo.path)">
              打开目录
            </button>
          </div>
        </div>
        <p class="repository-path">{{ repo.path }}</p>
        <div class="mapping-fields">
          <div class="mapping-branch">
            <span>开发分支</span
            ><code v-bind="branchCopyProps(repo.developmentBranch)" data-development>{{
              repo.developmentBranch
            }}</code>
          </div>
          <div v-if="draft.scmTestProjectId.trim()" class="mapping-branch">
            <span>测试分支</span
            ><code v-bind="branchCopyProps(repo.testBranch)" data-test>{{
              repo.testBranch || '未配置'
            }}</code>
          </div>
        </div>
        <div class="repository-state" data-local-branch>
          <span>当前本地分支</span
          ><code
            v-bind="
              branchCopyProps(statusFor(repo.id)?.error ? '' : statusFor(repo.id)?.currentBranch)
            "
            >{{ localBranch(repo.id) }}</code
          >
          <span v-if="statusFor(repo.id)?.dirty" class="warning">有未提交更改</span>
          <span v-if="statusFor(repo.id)?.merging" class="warning">合并中</span>
          <span v-if="statusFor(repo.id)?.rebasing" class="warning">变基中</span>
          <span v-if="statusFor(repo.id)?.error" class="error">{{
            statusFor(repo.id)?.error
          }}</span>
        </div>
      </div>
      <div
        v-for="({ module, testing: targets }, index) in moduleRows.filter(
          (row) => !row.repositories.length,
        )"
        :key="index"
        class="repository-unlinked"
      >
        <div class="repository-heading">
          <div class="repository-identity">
            <strong>{{ module.moduleName }}</strong
            ><span class="hint">未关联</span>
            <span class="repository-metadata"
              >SCM Git ID：<code data-scm-git-id>{{ module.gitId || '未提供' }}</code></span
            >
          </div>
          <button class="button small" :disabled="locked" @click="emit('edit')">
            关联本地目录
          </button>
        </div>
        <div class="mapping-fields">
          <div class="mapping-branch">
            <span>开发分支</span
            ><code v-bind="branchCopyProps(module.branch)" data-development>{{
              module.branch
            }}</code>
          </div>
          <div v-for="(target, targetIndex) in targets" :key="targetIndex" class="mapping-branch">
            <span>测试分支</span
            ><code v-bind="branchCopyProps(target.branch)" data-test>{{
              target.branch || '未配置'
            }}</code>
          </div>
          <div v-if="!targets.length" class="mapping-branch">
            <span>测试分支</span><code data-test>未配置</code>
          </div>
        </div>
      </div>
    </section>
    <section class="workbench-card" aria-label="Git 批量操作">
      <div class="section-heading git-actions-heading">
        <div class="git-actions-title">
          <h3>Git 批量操作</h3>
          <span class="hint">先获取远端并预检，核对计划后执行。</span>
        </div>
        <span class="hint">已选 {{ selectedRepositories.length }} 个仓库</span>
      </div>
      <p v-if="dirty" class="hint" data-scm-unsaved>SCM 配置有更改，请重新加载模块。</p>
      <div class="toolbar">
        <button
          class="button small"
          :disabled="!!switchDisabledReason"
          :title="switchDisabledReason || '获取远端并预检，确认计划后切换开发分支。'"
          @click="prepare('switch')"
        >
          切换开发分支</button
        ><button
          class="button small"
          :disabled="!!pushDevDisabledReason"
          :title="pushDevDisabledReason || '获取远端并预检，确认计划后推送开发分支到远程。'"
          @click="prepare('push-dev')"
        >
          推送开发分支到远程</button
        ><button
          class="button small"
          :disabled="!!switchTestDisabledReason"
          :title="
            switchTestDisabledReason ||
            '获取远端并预检，确认后切换到测试分支，仅快进同步远端最新代码。'
          "
          @click="prepare('switch-test')"
        >
          切换到测试分支</button
        ><button
          class="button small"
          :disabled="!!switchMasterDisabledReason"
          :title="
            switchMasterDisabledReason ||
            '获取远端并预检，确认后切换 master，仅快进同步远端最新代码。'
          "
          @click="prepare('switch-master')"
        >
          切换master分支</button
        ><button
          class="button small"
          :disabled="!!mergeDisabledReason"
          :title="mergeDisabledReason || '获取远端并预检，确认计划后合并到测试分支。'"
          @click="prepare('merge')"
        >
          合并到测试分支</button
        ><label class="checkbox"
          ><input v-model="push" type="checkbox" :disabled="locked" />合并成功后推送测试分支</label
        >
      </div>
      <div v-if="plan" class="plan">
        <h4 ref="planHeading" tabindex="-1">确认 Git 计划</h4>
        <p>
          {{ gitActionLabels[plan.operation] }} ·
          {{
            plan.operation === 'push-dev'
              ? '将推送开发分支'
              : plan.push
                ? '将推送测试分支'
                : '不推送远端'
          }}
        </p>
        <p v-if="plan.operation === 'switch-test' || plan.operation === 'switch-master'">
          切换后只允许快进同步：执行时重新获取远端，以当时最新提交为准；本地领先时保留提交，分叉时停止，不创建合并提交。
          有未提交修改时拒绝操作；无远端的本地仓库仅切换，不进行远程同步。
        </p>
        <p v-if="plan.operation === 'merge'">可逐处选择 pom.xml 冲突内容，其他冲突需手工处理。</p>
        <ul>
          <li v-for="item in plan.repositories" :key="item.repository.id">
            <strong>{{ item.repository.name }}</strong>
            <div class="plan-branch">
              <span>当前分支：</span><code>{{ item.currentBranch }}</code>
            </div>
            <div v-if="plan.operation === 'push-dev'" class="plan-branch">
              <span>本地开发分支：</span><code>{{ item.repository.developmentBranch }}</code>
            </div>
            <div class="plan-branch">
              <span>{{ plan.operation === 'push-dev' ? '目标远程分支：' : '目标分支：' }}</span>
              <code>{{
                plan.operation === 'push-dev'
                  ? `${item.repository.remote}/${item.targetBranch}`
                  : item.targetBranch
              }}</code>
            </div>
            <p
              v-if="plan.operation === 'merge' && item.developmentRemoteSha !== undefined"
              data-development-sync
            >
              <span v-if="item.syncDevelopmentBranch" class="warning">
                开发分支落后于远端，确认执行后先快进同步开发分支，再合并到测试分支。
                后续合并失败或中止时，已完成的开发分支快进不会回滚。
              </span>
              <span v-else-if="!item.developmentRemoteSha">
                远端未找到同名开发分支，本次使用本地已提交的开发代码。
              </span>
              <span v-else-if="item.sourceSha !== item.developmentRemoteSha" class="warning">
                开发分支已有本地未推送提交，将保留并参与本次合并。
              </span>
              <span v-else>开发分支已与远端同步。</span>
            </p>
            <span v-if="plan.operation === 'merge' && item.syncTestBranch" class="warning">
              本地测试分支落后于远端，执行时将先快进同步远端最新代码，再合并开发分支。
            </span>
            <span
              v-else-if="
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
      <div v-if="desktop" class="toolbar history-toolbar">
        <label>
          待处理操作
          <select v-model="selectedHistoryId" :disabled="locked || !history.length">
            <option value="" disabled>
              {{ history.length ? '选择待处理操作' : '暂无待处理操作' }}
            </option>
            <option v-for="item in history" :key="item.planId" :value="item.planId">
              {{ historyLabel(item) }}
            </option>
          </select>
        </label>
        <div class="toolbar">
          <button class="button small" :disabled="locked" @click="refreshHistory()">
            刷新记录
          </button>
          <button
            class="button small"
            :disabled="locked"
            :aria-expanded="confirmingClearHistory"
            @click="confirmingClearHistory = true"
          >
            清除记录
          </button>
        </div>
      </div>
      <div
        v-if="confirmingClearHistory"
        class="history-confirm"
        role="group"
        aria-label="确认清除 Git 操作记录"
      >
        <p class="warning">
          将清除当前项目已结束且无需恢复的 Git
          操作记录，以及当前选中且已执行、已暂停、没有保留工作区的操作。选中的待处理操作一经清除便无法续跑，此操作无法撤销。运行中、仍需恢复的未选中操作和有保留工作区的操作将保留。
        </p>
        <div class="toolbar">
          <button
            class="button small"
            :disabled="clearingHistory"
            @click="confirmingClearHistory = false"
          >
            取消清除
          </button>
          <button class="button small danger" :disabled="locked" @click="clearHistory">
            确认清除
          </button>
        </div>
        <p v-if="clearingHistory" class="hint" role="status">正在清除记录…</p>
      </div>
      <div v-if="operation" class="operation">
        <div class="section-heading">
          <h4>执行结果 · {{ operationLabels[operation.status] }}</h4>
          <button
            class="button small"
            v-if="operation.status === 'running'"
            :disabled="!!activity"
            @click="cancel"
          >
            取消任务
          </button>
          <button
            v-if="canContinue"
            class="button small"
            :disabled="locked || dirty"
            title="只重新预检未执行的仓库，确认计划后执行"
            @click="prepareRemaining"
          >
            继续
          </button>
        </div>
        <p class="hint">{{ operationSummary(operation) }}</p>
        <p v-if="operation.status === 'paused'" class="warning">
          操作已暂停，已成功的仓库不会回滚。{{
            canContinue ? '点击「继续」重新预检未执行的仓库。' : ''
          }}
        </p>
        <p v-if="operation.repositories.some((item) => item.worktreePath)" class="warning">
          有保留的工作树，请按下方提示处理。
        </p>
        <div v-for="item in operation.repositories" :key="item.repoId" class="result">
          <div class="result-heading">
            <strong>{{ repositoryName(item.repoId) }}</strong>
            <span :class="item.status === 'error' ? 'error' : 'hint'">{{
              repositoryStatus(item)
            }}</span>
          </div>
          <p>{{ repositoryMessage(item) }}</p>
          <div
            v-if="item.worktreePath && (item.pomConflictFiles?.length || item.pomConflict)"
            class="pom-conflict"
          >
            <div v-for="file in item.pomConflictFiles" :key="file.path" class="pom-file">
              <strong
                ><code>{{ file.path }}</code></strong
              >
              <div
                v-for="block in file.blocks"
                :key="block.line"
                :data-pom-block="block.line"
                class="pom-block"
              >
                <p v-if="block.context" class="pom-context">{{ block.context }}</p>
                <p class="hint">合并冲突 · 工作树第 {{ block.line }} 行</p>
                <div class="pom-versions">
                  <section>
                    <strong>测试分支</strong>
                    <pre v-if="block.testContent" class="pom-test" tabindex="0">{{
                      block.testContent
                    }}</pre>
                    <p v-else class="hint">此侧无内容</p>
                    <label v-if="item.pomBlocksResolution" class="checkbox">
                      <input
                        v-model="pomChoices[pomChoiceKey(item.repoId, file.path, block.line)]"
                        :name="pomChoiceKey(item.repoId, file.path, block.line)"
                        type="radio"
                        value="test"
                        :disabled="locked || operation.status !== 'paused'"
                      />采用此处测试分支
                    </label>
                  </section>
                  <section>
                    <strong>开发分支</strong>
                    <pre v-if="block.developmentContent" class="pom-development" tabindex="0">{{
                      block.developmentContent
                    }}</pre>
                    <p v-else class="hint">此侧无内容</p>
                    <label v-if="item.pomBlocksResolution" class="checkbox">
                      <input
                        v-model="pomChoices[pomChoiceKey(item.repoId, file.path, block.line)]"
                        :name="pomChoiceKey(item.repoId, file.path, block.line)"
                        type="radio"
                        value="development"
                        :disabled="locked || operation.status !== 'paused'"
                      />采用此处开发分支
                    </label>
                  </section>
                </div>
              </div>
              <details v-if="file.branchDiff" open class="pom-branch-diff">
                <summary>两分支的全部差异</summary>
                <p class="hint">− 测试分支，+ 开发分支。每组开头标出两侧文件的行号。</p>
                <pre tabindex="0" class="pom-diff"><span
                  v-for="(line, index) in file.branchDiff.replace(/\n$/, '').split('\n')"
                  :key="index"
                  :class="{
                    'pom-diff-range': line.startsWith('@@ '),
                    'pom-test': line.startsWith('-'),
                    'pom-development': line.startsWith('+'),
                  }"
                ><template v-if="file.diffContexts?.[line]"><strong class="pom-context">{{ file.diffContexts[line] }}</strong>{{ '  ' }}</template><span class="pom-diff-text">{{ line }}</span></span></pre>
              </details>
            </div>
            <template v-if="item.pomBlocksResolution">
              <p class="hint">
                {{
                  item.pomBlocksResolution.manualRemaining
                    ? '应用后仍需在工作树处理其余冲突。'
                    : '选齐后统一应用并保留 Git 已自动合并的内容。成功后继续原批次。'
                }}
              </p>
              <button
                class="button small"
                :disabled="locked || operation.status !== 'paused' || !selectedPomChoices(item)"
                @click="applyPomChoices(item)"
              >
                {{
                  item.pomBlocksResolution.manualRemaining
                    ? '应用所选 POM 内容'
                    : item.pomBlocksResolution.push
                      ? '应用所选内容、提交并推送'
                      : '应用所选内容并提交'
                }}
              </button>
            </template>
            <p v-else-if="!item.pomConflict" class="hint">请打开工作树，按文件和行号手工合并。</p>
            <template v-else-if="item.pomConflict">
              <p>
                <code>{{ item.pomConflict.path }}</code> · 选择要保留的整份文件
              </p>
              <p class="hint">采用一侧的整份 POM 会舍弃另一侧独有的改动。</p>
              <p class="hint">
                {{
                  item.pomConflict.push
                    ? '选定后提交并推送到测试分支。'
                    : '选定后提交到本地测试分支。'
                }}
              </p>
              <div class="pom-versions">
                <section>
                  <strong>测试分支 · {{ item.pomConflict.testBranch }}</strong>
                  <details :open="!item.pomConflictFiles?.length">
                    <summary>查看完整 POM</summary>
                    <pre tabindex="0">{{ item.pomConflict.testContent }}</pre>
                  </details>
                  <button
                    class="button small"
                    :disabled="locked"
                    @click="resolvePomConflict(item.repoId, 'test')"
                  >
                    采用测试分支版本
                  </button>
                </section>
                <section>
                  <strong>开发分支 · {{ item.pomConflict.developmentBranch }}</strong>
                  <details :open="!item.pomConflictFiles?.length">
                    <summary>查看完整 POM</summary>
                    <pre tabindex="0">{{ item.pomConflict.developmentContent }}</pre>
                  </details>
                  <button
                    class="button small"
                    :disabled="locked"
                    @click="resolvePomConflict(item.repoId, 'development')"
                  >
                    采用开发分支版本
                  </button>
                </section>
              </div>
            </template>
          </div>
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
              <button class="button small" @click="openPath(item.worktreePath)">打开工作树</button>
              <button
                class="button small"
                :disabled="locked"
                @click="openWorktreeInIde(item.repoId, 'IDEA')"
              >
                在 IDEA 中打开
              </button>
              <button
                class="button small"
                :disabled="locked"
                @click="openWorktreeInIde(item.repoId, 'WebStorm')"
              >
                在 WebStorm 中打开工作树
              </button>
              <button
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
              >请在工作树中解决冲突并提交。确认干净后，点击「完成处理」继续原批次。手工提交需自行推送。</small
            ></template
          >
        </div>
      </div>
    </section>
  </section>
</template>

<style scoped>
.pom-conflict {
  margin-block: 10px;
}
.pom-versions {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 260px), 1fr));
  gap: 12px;
}
.pom-file + .pom-file,
.pom-block + .pom-block {
  margin-top: 12px;
}
.pom-versions section {
  min-width: 0;
}
.pom-versions pre {
  max-height: 250px;
  overflow: auto;
  border: 1px solid var(--line);
  padding: 8px;
}
.pom-branch-diff {
  margin-block: 12px;
}
.pom-diff {
  max-height: 320px;
  overflow: auto;
  border: 1px solid var(--line);
}
.pom-diff > span {
  display: block;
  min-width: max-content;
  padding-inline: 8px;
}
.pom-diff-range {
  color: var(--muted);
  background: color-mix(in srgb, var(--line) 45%, transparent);
}
.pom-test {
  border-inline-start: 3px solid #bf711e;
  background: color-mix(in srgb, #bf711e 14%, transparent);
}
.pom-development {
  border-inline-start: 3px solid #378366;
  background: color-mix(in srgb, #378366 14%, transparent);
}

.backend-workbench {
  display: grid;
  gap: 14px;
  color: var(--ink);
}
.section-heading,
.repository-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
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
.git-actions-heading,
.git-actions-title {
  flex-wrap: wrap;
}
.git-actions-title {
  display: flex;
  align-items: baseline;
  gap: 6px 12px;
  min-width: 0;
}
.git-actions-title h3 {
  flex-shrink: 0;
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
  margin: 0 0 10px 23px;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 10px;
}
.mapping-branch {
  min-width: 0;
}
.repository-identity {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 14px;
  min-width: 0;
}
.repository-metadata {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 6px;
  color: var(--muted);
  font-size: 11px;
}
.repository-metadata code {
  min-width: 0;
  color: var(--ink);
}
.mapping-branch {
  display: grid;
  color: var(--text-secondary);
  font-size: 12px;
  grid-column: 1 / -1;
  grid-template-columns: 56px minmax(0, 1fr);
  align-items: center;
  gap: 8px;
}
.mapping-branch code {
  min-width: 0;
  max-width: 100%;
  justify-self: start;
  padding: 3px 7px;
  border-radius: 4px;
  user-select: text;
}
/* 分支角色使用固定配色，切换主题后仍能保持一致的辨识。 */
.mapping-branch [data-development] {
  color: #2458ce;
  background: #edf3ff;
}
.mapping-branch [data-test] {
  color: #a34b0b;
  background: #fff1df;
}
.repository,
.repository-unlinked {
  border-top: 1px solid var(--line);
  padding-top: 12px;
  margin-top: 12px;
}
.repository-unlinked .repository-heading {
  margin-bottom: 10px;
  padding-left: 23px;
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
  min-width: 0;
  max-width: 100%;
  white-space: normal;
  color: var(--success-text);
  background: var(--success-soft);
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
small {
  display: block;
  margin-top: 5px;
}
.branch-copyable {
  /* 系统 copy 指针在 macOS 显示加号，改用双页图标统一复制提示。 */
  cursor:
    url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'%3E%3Crect x='3' y='3' width='12' height='14' rx='2' fill='white' stroke='%23333' stroke-width='1.5'/%3E%3Crect x='8' y='7' width='12' height='14' rx='2' fill='white' stroke='%23333' stroke-width='1.5'/%3E%3C/svg%3E")
      3 3,
    pointer;
}
.branch-copyable:hover {
  text-decoration: underline;
  text-underline-offset: 3px;
}
.branch-copyable:focus-visible {
  outline: 2px solid var(--accent-border);
  outline-offset: 2px;
}
code {
  font-family: var(--mono);
  font-size: 11px;
  white-space: normal;
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
.plan-branch {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  align-items: baseline;
  gap: 6px;
  margin-top: 5px;
  font-size: 12px;
}
.plan-branch code {
  min-width: 0;
}
.history-toolbar {
  align-items: flex-end;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--line);
}
.history-toolbar label {
  min-width: 0;
  flex: 1 1 15rem;
}
.history-toolbar select {
  height: var(--control-height-small);
  min-height: var(--control-height-small);
}
.history-confirm {
  margin-top: 8px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 7px;
  font-size: 12px;
}
.history-confirm p {
  margin: 0 0 8px;
}
.history-confirm [role='status'] {
  margin: 8px 0 0;
}
.operation {
  margin-top: 16px;
}
.result {
  border-top: 1px solid var(--line);
  padding: 12px 0;
  font-size: 12px;
}
.result-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.result-heading strong {
  min-width: 0;
  overflow-wrap: anywhere;
}
.result-heading > span {
  flex-shrink: 0;
  padding: 3px 8px;
  border: 1px solid var(--line);
  border-radius: 4px;
}
.result .toolbar {
  margin-top: 8px;
}
@media (max-width: 760px) {
  .mapping-fields,
  .scm-projects {
    grid-template-columns: 1fr 1fr;
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
