import { invoke, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { downloadDir, join } from '@tauri-apps/api/path';
import type {
  AppConfig,
  Project,
  ManifestInfo,
  Tools,
  Action,
  Settings,
  BuildMode,
  NodeVersion,
  UniappScript,
  EnvironmentReport,
  BuildRecord,
  BuildHistorySettings,
  BuildHistorySettingsUpdate,
  BackendCandidate,
  BackendRepository,
  BackendRepositoryStatus,
  BackendGitPlan,
  BackendGitOperation,
  PomBlockChoice,
  LogMcpStatus,
  SqlpadExecutionDetail,
  SqlpadExecutionSummary,
  SqlpadPendingApproval,
  ScmPendingApproval,
  SqlpadStatus,
  McpCatalogTool,
  McpClient,
  McpClientStatus,
  ScmStatus,
  ScmProject,
  ScmCreateProjectOptions,
  ScmCreateProjectInput,
  ScmEditProjectOptions,
  ScmModule,
} from './types';
import { defaultConfig, normalizeProjectPlatforms } from './types';

export type LogMcpScanScope = 'first' | 'all';
export const desktop = isTauri();
// 浏览器预览使用独立本地存储；构建和外部程序调用始终只在桌面应用中执行。
const PREVIEW_KEY = 'dev-dock-preview-v1';
export const api = {
  async load(): Promise<AppConfig> {
    if (desktop) return invoke('load_config');
    const raw = localStorage.getItem(PREVIEW_KEY) ?? localStorage.getItem('ai-dev-preview-v1');
    const config = raw ? JSON.parse(raw) : defaultConfig();
    // 仅首次读取时复制旧预览配置，保留旧值且不覆盖已有的新配置。
    if (raw && localStorage.getItem(PREVIEW_KEY) === null) {
      localStorage.setItem(PREVIEW_KEY, raw);
    }
    // 旧版浏览器预览配置没有主题和目录开关，补默认值而不改动项目或工具路径。
    config.settings = { ...defaultConfig().settings, ...config.settings };
    config.projects = config.projects.map((project: Project) =>
      normalizeProjectPlatforms({
        ...project,
        nodeVersion: project.nodeVersion ?? '',
        nvmDir: project.nvmDir ?? '',
        envVars: project.envVars ?? '',
        aiToolId: project.aiToolId ?? '',
      }),
    );
    return config;
  },
  async persist(config: AppConfig) {
    if (desktop) await invoke('save_config', { config });
    else localStorage.setItem(PREVIEW_KEY, JSON.stringify(config));
  },
  scanBackendChildren(parentPath: string) {
    return native<BackendCandidate[]>('scan_backend_children', { parentPath });
  },
  backendRepositoryRemoteUrls(repositories: BackendRepository[]) {
    return native<Record<string, string>>('backend_repository_remote_urls', {
      repositories: JSON.parse(JSON.stringify(repositories)),
    });
  },
  backendRepositoryStatuses(repositories: BackendRepository[]) {
    return native<BackendRepositoryStatus[]>('backend_repository_statuses', {
      repositories: JSON.parse(JSON.stringify(repositories)),
    });
  },
  prepareBackendGit(
    projectId: string,
    repositories: BackendRepository[],
    operation: BackendGitPlan['operation'],
    push = false,
    keepTestPom = false,
  ) {
    return native<BackendGitPlan>('prepare_backend_git', {
      projectId,
      repositories: JSON.parse(JSON.stringify(repositories)),
      operation,
      push,
      keepTestPom,
    });
  },
  prepareRemainingBackendGit(planId: string) {
    return native<BackendGitPlan>('prepare_remaining_backend_git', { planId });
  },
  executeBackendGit(planId: string) {
    return native<string>('execute_backend_git', { planId });
  },
  backendGitState(planId: string) {
    return native<BackendGitOperation>('backend_git_state', { planId });
  },
  backendGitOperations(projectId: string) {
    return native<BackendGitOperation[]>('backend_git_operations', { projectId });
  },
  backendClearGitOperations(projectId: string, selectedPlanId?: string) {
    return native<string[]>(
      'clear_backend_git_operations',
      selectedPlanId === undefined ? { projectId } : { projectId, selectedPlanId },
    );
  },
  resolveBackendPomConflict(planId: string, repoId: string, side: 'test' | 'development') {
    return native<void>('resolve_backend_pom_conflict', { planId, repoId, side });
  },
  resolveBackendPomBlocks(planId: string, repoId: string, choices: PomBlockChoice[]) {
    return native<void>('resolve_backend_pom_blocks', { planId, repoId, choices });
  },
  openBackendWorktreeInIdea(planId: string, repoId: string) {
    return native<void>('open_backend_worktree_in_idea', { planId, repoId });
  },
  openBackendWorktreeInWebStorm(planId: string, repoId: string) {
    return native<void>('open_backend_worktree_in_webstorm', { planId, repoId });
  },
  completeBackendGit(planId: string, repoId: string) {
    return native<void>('complete_backend_git', { planId, repoId });
  },
  abortBackendGit(planId: string, repoId: string) {
    return native<void>('abort_backend_git', { planId, repoId });
  },
  mcpToolCatalog() {
    return native<McpCatalogTool[]>('mcp_tool_catalog');
  },
  sqlpadExecutionHistory() {
    return native<SqlpadExecutionSummary[]>('sqlpad_execution_history');
  },
  sqlpadExecutionDetail(id: string) {
    return native<SqlpadExecutionDetail>('sqlpad_execution_detail', { id });
  },
  sqlpadHistoryLimit() {
    return native<number>('sqlpad_history_limit');
  },
  sqlpadSetHistoryLimit(limit: number) {
    return native<number>('sqlpad_set_history_limit', { limit });
  },
  sqlpadDeleteExecution(id: string) {
    return native<void>('sqlpad_delete_execution', { id });
  },
  sqlpadClearHistory() {
    return native<void>('sqlpad_clear_history');
  },
  sqlpadRerunExecution(id: string) {
    return native<void>('sqlpad_rerun_execution', { id });
  },
  listScmProjectApprovals() {
    return native<ScmPendingApproval[]>('list_scm_project_approvals');
  },
  resolveScmProjectApproval(requestId: string, approved: boolean) {
    return native<void>('resolve_scm_project_approval', { requestId, approved });
  },
  sqlpadPendingApproval() {
    return native<SqlpadPendingApproval | null>('sqlpad_pending_approval');
  },
  sqlpadDecideApproval(requestId: string, approved: boolean) {
    return native<void>('sqlpad_decide_approval', { requestId, approved });
  },
  sqlpadStatus() {
    return native<SqlpadStatus>('sqlpad_status');
  },
  sqlpadLogin(baseUrl: string, email: string, password: string, remember: boolean) {
    return native<SqlpadStatus>('sqlpad_login', { baseUrl, email, password, remember });
  },
  sqlpadSetAutoApprove(connectionId: string, enabled: boolean) {
    return native<SqlpadStatus>('sqlpad_set_auto_approve', { connectionId, enabled });
  },
  sqlpadSelectConnection(connectionId: string) {
    return native<SqlpadStatus>('sqlpad_select_connection', { connectionId });
  },
  sqlpadLogout() {
    return native<SqlpadStatus>('sqlpad_logout');
  },
  logMcpStatus() {
    return native<LogMcpStatus>('log_mcp_status');
  },
  logMcpRawStatus() {
    return native<boolean>('log_mcp_raw_status');
  },
  logMcpSetRaw(enabled: boolean) {
    return native<boolean>('log_mcp_set_raw', { enabled });
  },
  logMcpMatchLimit() {
    return native<number>('log_mcp_match_limit');
  },
  logMcpSetMatchLimit(limit: number) {
    return native<number>('log_mcp_set_match_limit', { limit });
  },
  logMcpScanScope() {
    return native<LogMcpScanScope>('log_mcp_scan_scope');
  },
  logMcpSetScanScope(scope: LogMcpScanScope) {
    return native<LogMcpScanScope>('log_mcp_set_scan_scope', { scope });
  },
  logMcpEnable() {
    return native<LogMcpStatus>('log_mcp_enable');
  },
  logMcpDisable() {
    return native<LogMcpStatus>('log_mcp_disable');
  },
  logMcpToken() {
    return native<string>('log_mcp_token');
  },
  mcpClientStatus(client: McpClient) {
    return native<McpClientStatus>('mcp_client_status', { client });
  },
  mcpClientConnect(client: McpClient) {
    return native<McpClientStatus>('mcp_client_connect', { client });
  },
  mcpClientDisconnect(client: McpClient) {
    return native<McpClientStatus>('mcp_client_disconnect', { client });
  },
  mcpClientTemplate() {
    return native<string>('mcp_client_template');
  },
  logScmStatus() {
    return native<ScmStatus>('log_scm_status');
  },
  logScmLogin(baseUrl: string, username: string, password: string, remember: boolean) {
    return native<ScmStatus>('log_scm_login', { baseUrl, username, password, remember });
  },
  logScmLogout() {
    return native<ScmStatus>('log_scm_logout');
  },
  scmStatus() {
    return native<ScmStatus>('scm_status');
  },
  scmLogin(baseUrl: string, username: string, password: string, remember: boolean) {
    return native<ScmStatus>('scm_login', { baseUrl, username, password, remember });
  },
  scmLogout() {
    return native<ScmStatus>('scm_logout');
  },
  scmProjects() {
    return native<ScmProject[]>('scm_projects');
  },
  scmCreateProjectOptions() {
    return native<ScmCreateProjectOptions>('scm_create_project_options');
  },
  scmCreateProject(input: ScmCreateProjectInput) {
    return native<string>('scm_create_project', {
      input: { ...input, modules: [...input.modules] },
    });
  },
  scmEditProjectOptions(projectId: string) {
    return native<ScmEditProjectOptions>('scm_edit_project_options', { projectId });
  },
  scmUpdateProject(
    projectId: string,
    input: ScmCreateProjectInput,
    authors: string[],
    revision: string,
  ) {
    return native<void>('scm_update_project', {
      projectId,
      input: { ...input, modules: [...input.modules] },
      authors: [...authors],
      revision,
    });
  },
  scmProjectModules(projectId: string) {
    return native<ScmModule[]>('scm_project_modules', { projectId });
  },
  checkEnvironment(project: Project, settings: Settings) {
    return native<EnvironmentReport>('check_environment', {
      project: JSON.parse(JSON.stringify(project)),
      settings: JSON.parse(JSON.stringify(settings)),
    });
  },
  requestAccessibilityPermission() {
    return native<boolean>('request_accessibility_permission');
  },
  buildHistorySettings() {
    return native<BuildHistorySettings>('get_build_history_settings');
  },
  saveBuildHistorySettings(retentionLimit: number) {
    return native<BuildHistorySettingsUpdate>('save_build_history_settings', { retentionLimit });
  },
  buildHistory(projectId: string) {
    return native<BuildRecord[]>('list_build_history', { projectId });
  },
  deleteBuildHistory(projectId: string, id: string) {
    return native<void>('delete_build_history', { projectId, id });
  },
  gitBranch(path: string) {
    return native<string>('read_git_branch', { path });
  },
  manifest(project: Project) {
    return native<ManifestInfo>('read_manifest', { project });
  },
  detect() {
    return native<Tools>('detect_tools');
  },
  nodeVersions(options?: { manager?: 'nvm' | 'fnm'; dir?: string }) {
    return native<NodeVersion[]>(
      'detect_node_versions',
      options ? { manager: options.manager ?? null, dir: options.dir?.trim() || null } : undefined,
    );
  },
  packageScripts(project: Project) {
    return native<{ name: string; command: string }[]>('read_package_scripts', { project });
  },
  startBuild(project: Project, mode: BuildMode, versionName: string, versionCode: string) {
    return native<string>('start_build', { project, mode, versionName, versionCode });
  },
  runAction(project: Project, action: Action, settings: Settings) {
    return native<string>('run_action', { project, action, settings });
  },
  openProjectTerminal(project: Project, toolId?: string) {
    return native<string>('open_project_terminal', { project, toolId: toolId ?? null });
  },
  openProjectCommand(project: Project) {
    return native<string>('open_project_command', { project });
  },
  cancel(taskId: string) {
    return native<void>('cancel_task', { taskId });
  },
  showMain(projectId?: string) {
    return native<void>('show_main_window', { projectId: projectId ?? null });
  },
  hideQuickPanel() {
    return native<void>('hide_quick_panel');
  },
  openPath(path: string) {
    return native<void>('open_path', { path });
  },
  menus(hbuilderxPath: string) {
    return native<string[][]>('list_menus', { hbuilderxPath });
  },
  uniappScripts(project: Project) {
    return native<UniappScript[]>('read_uniapp_scripts', { project });
  },
  recover() {
    return native<string[]>('recover_pending');
  },
  async choosePath(directory = true): Promise<string | null> {
    if (!desktop) throw new Error('文件选择需要在桌面应用中使用；预览时可手动填写路径。');
    const selected = await open({ directory, multiple: false });
    return typeof selected === 'string' ? selected : null;
  },
  async exportBackup(config: AppConfig): Promise<boolean> {
    if (!desktop) {
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(config, null, 2)], { type: 'application/json' }),
      );
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`;
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      return true;
    }
    let defaultPath = `dev-dock-backup-${new Date().toISOString().slice(0, 10)}.json`;
    try {
      defaultPath = await join(await downloadDir(), defaultPath);
    } catch {
      // 系统未提供下载目录时，仍允许通过保存对话框自行选择位置。
    }
    const path = await save({
      defaultPath,
      filters: [{ name: 'JSON 备份', extensions: ['json'] }],
    });
    if (!path) return false;
    await invoke('write_backup', { path, config });
    return true;
  },
  async importBackup(): Promise<AppConfig | null> {
    if (!desktop) throw new Error('请在桌面应用中导入备份，以完成配置校验。');
    const path = await open({
      multiple: false,
      filters: [{ name: 'JSON 备份', extensions: ['json'] }],
    });
    if (typeof path !== 'string') return null;
    return invoke('read_backup', { path });
  },
};
function native<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!desktop)
    return Promise.reject(new Error('当前是浏览器预览，请在 DevDock 桌面应用中执行此操作。'));
  return invoke<T>(command, args);
}
