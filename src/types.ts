import { isWindows, pathKey } from './platform';

export type ProjectType = 'uniapp' | 'nodejs' | 'backend';
export type Platform = 'app' | 'web' | 'miniapp';
export type BuildMode = 'apk' | 'wgt-android' | 'wgt-ios';
export interface Action {
  id: string;
  name: string;
  platform: string;
  kind: 'menu' | 'shell';
  menuPath: string[];
  command: string;
  envVars: string;
}
export interface BackendRepository {
  id: string;
  name: string;
  path: string;
  remote: string;
  scmGitId: string;
  scmModuleName: string;
  developmentBranch: string;
  testBranch: string;
}
export interface BackendCandidate {
  id: string;
  name: string;
  path: string;
  isGit: boolean;
  currentBranch: string;
  error: string;
}
export interface BackendRepositoryStatus {
  id: string;
  currentBranch: string;
  head: string;
  dirty: boolean;
  merging: boolean;
  rebasing: boolean;
  remoteUrl: string;
  branches: string[];
  root: string;
  error: string;
}
export interface BackendGitPlanRepository {
  repository: BackendRepository;
  root: string;
  commonDir: string;
  currentBranch: string;
  sourceSha: string;
  targetBranch: string;
  targetSha: string;
  createTracking: boolean;
  remoteSha: string;
}
export interface BackendGitPlan {
  planId: string;
  projectId: string;
  operation: 'switch' | 'merge';
  push: boolean;
  keepTestPom?: boolean;
  repositories: BackendGitPlanRepository[];
}
export interface BackendGitOperation {
  planId: string;
  taskId: string;
  status: 'prepared' | 'running' | 'success' | 'paused' | 'cancelled' | 'aborting';
  repositories: {
    repoId: string;
    status: 'pending' | 'running' | 'success' | 'error' | 'cancelled' | 'aborted';
    message: string;
    worktreePath: string;
  }[];
}
export interface ScmStatus {
  connected: boolean;
  baseUrl: string | null;
  username: string | null;
  remembered: boolean;
  message: string | null;
}
export interface ScmProject {
  id: string;
  name: string;
  status: string | null;
}
export interface ScmModule {
  gitId: string | null;
  moduleName: string;
  gitRemoteUrl: string | null;
  branch: string;
}
export interface Project {
  id: string;
  name: string;
  type: ProjectType;
  path: string;
  platforms: string[];
  manifestPath: string;
  androidAppId: string;
  iosAppId: string;
  androidProjectPath: string;
  androidModule: string;
  jdkPath: string;
  gradleTask: string;
  outputDir: string;
  syncMainVersion: boolean;
  terminal: string;
  nodeVersion: string;
  nvmDir: string;
  envVars: string;
  aiToolId: string;
  commands: Action[];
  subProjects: BackendRepository[];
  scmDevelopmentProjectId: string;
  scmTestProjectId: string;
}
export interface NodeVersion {
  version: string;
  path: string;
  nvmDir: string;
}
export const THEME_OPTIONS = [
  {
    id: 'lavender',
    name: '浅紫微光',
    english: 'LAVENDER',
    description: '清透紫色与柔白底色，轻盈而鲜明。',
    color: '#6941c6',
  },
  {
    id: 'blue',
    name: '深海蓝',
    english: 'OCEAN',
    description: '明亮钴蓝搭配深海侧栏，干净利落。',
    color: '#2563eb',
  },
  {
    id: 'cyan',
    name: '松石青',
    english: 'JADE',
    description: '松石青绿与薄荷白，清新而沉静。',
    color: '#0f766e',
  },
  {
    id: 'rose',
    name: '玫瑰粉',
    english: 'ROSE',
    description: '鲜明玫瑰色与浅樱白，柔和有层次。',
    color: '#be185d',
  },
  {
    id: 'amber',
    name: '琥珀橙',
    english: 'AMBER',
    description: '温暖琥珀色与奶油白，明快有质感。',
    color: '#b45309',
  },
  {
    id: 'slate',
    name: '柔雅黑',
    english: 'CHARCOAL',
    description: '柔和炭黑与白色卡片，清晰而不生硬。',
    color: '#303030',
  },
] as const;
export type ThemeName = (typeof THEME_OPTIONS)[number]['id'];
export interface AiTool {
  id: string;
  name: string;
  command: string;
  enabled: boolean;
}
export interface Settings {
  hbuilderxPath: string;
  terminal: string;
  jdkPath: string;
  theme: ThemeName;
  openOutputAfterBuild: boolean;
  aiTools: AiTool[];
  defaultAiToolId: string;
}
export interface AppConfig {
  schemaVersion: 1;
  projects: Project[];
  settings: Settings;
}
export interface UniappScript {
  id: string;
  title: string;
  uniPlatform: string;
  platform: string;
  runMenuPath: string[];
  publishMenuPath: string[];
}
export interface ManifestInfo {
  versionName: string;
  versionCode: string;
  appid: string;
}
export interface Tool {
  name: string;
  path: string;
}
export interface Tools {
  hbuilderxPath: string;
  terminals: Tool[];
  jdks: Tool[];
}
export type TaskStatus = 'running' | 'success' | 'error' | 'cancelled' | 'triggered';
export interface TaskEvent {
  taskId: string;
  projectId: string;
  kind: 'log' | 'status' | 'artifact' | 'notice';
  message: string;
  status?: TaskStatus;
  path?: string;
}
export interface EnvironmentCheckItem {
  id: string;
  label: string;
  status: 'pass' | 'warning' | 'error' | 'skipped';
  detail: string;
  action: 'request-accessibility' | 'settings' | null;
}
export interface EnvironmentReport {
  projectId: string;
  checkedAt: number;
  items: EnvironmentCheckItem[];
}
export interface BuildHistorySettings {
  retentionLimit: number;
}
export interface BuildHistorySettingsUpdate extends BuildHistorySettings {
  removedCount: number;
}
export interface BuildRecord {
  id: string;
  projectId: string;
  projectName: string;
  projectPath: string;
  mode: BuildMode;
  versionName: string;
  versionCode: string;
  branch: string | null;
  startedAt: number;
  finishedAt: number | null;
  durationMs: number | null;
  status: 'running' | 'success' | 'error' | 'cancelled' | 'interrupted';
  summary: string;
  artifacts: { path: string; exists: boolean }[];
}
export interface Task {
  id: string;
  projectId: string;
  title: string;
  status: TaskStatus;
  logs: { time: string; message: string; kind: string }[];
  artifacts: string[];
}
export const PLATFORM_LABELS: Record<string, string> = {
  app: 'App 构建',
  web: 'Web',
  miniapp: '小程序',
};
export const TYPE_LABELS: Record<ProjectType, string> = {
  uniapp: 'uni-app',
  nodejs: 'Node.js',
  backend: '后端项目',
};
export function defaultConfig(): AppConfig {
  return {
    schemaVersion: 1,
    projects: [],
    settings: {
      hbuilderxPath: isWindows ? '' : '/Applications/HBuilderX.app',
      terminal: isWindows ? 'PowerShell' : '/Applications/Ghostty.app',
      jdkPath: '',
      theme: 'lavender',
      openOutputAfterBuild: true,
      aiTools: [],
      defaultAiToolId: '',
    },
  };
}
export function newProject(): Project {
  return {
    id: crypto.randomUUID(),
    name: '',
    type: 'uniapp',
    path: '',
    platforms: ['app'],
    manifestPath: '',
    androidAppId: '',
    iosAppId: '',
    androidProjectPath: '',
    androidModule: 'simpleDemo',
    jdkPath: '',
    gradleTask: ':simpleDemo:assembleRelease',
    outputDir: '',
    syncMainVersion: false,
    terminal: '',
    nodeVersion: '',
    nvmDir: '',
    envVars: '',
    aiToolId: '',
    commands: [],
    subProjects: [],
    scmDevelopmentProjectId: '',
    scmTestProjectId: '',
  };
}

export function normalizePlatform(platform: string): string {
  if (platform === 'pc') return 'web';
  if (platform === 'weixin' || platform === 'dingtalk') return 'miniapp';
  return platform;
}
export function normalizeProjectPlatforms(project: Project): Project {
  // 仅合并用途分类，具体菜单、命令和操作 ID 均保留。
  return {
    ...project,
    subProjects: (project.subProjects ?? []).map((repository) => ({
      id: repository.id,
      name: repository.name,
      path: repository.path,
      remote: repository.remote ?? 'origin',
      scmGitId: repository.scmGitId ?? '',
      scmModuleName: repository.scmModuleName ?? '',
      developmentBranch: repository.developmentBranch ?? '',
      testBranch: repository.testBranch ?? '',
    })),
    scmDevelopmentProjectId: project.scmDevelopmentProjectId ?? '',
    scmTestProjectId: project.scmTestProjectId ?? '',
    platforms: [...new Set(project.platforms.map(normalizePlatform))],
    commands: project.commands.map((action) => ({
      ...action,
      platform: normalizePlatform(action.platform),
      menuPath: [...action.menuPath],
      // 旧预览或备份缺少命令级环境变量时补空值。
      envVars: action.envVars || '',
    })),
  };
}

// 合并按 ID 和标准化路径去重，保留当前设备设置，避免导入覆盖本机工具路径。
export function mergeConfig(current: AppConfig, incoming: AppConfig): AppConfig {
  const canonicalPath = pathKey;
  const projects = structuredClone(current.projects).map(normalizeProjectPlatforms);
  for (const project of incoming.projects.map(normalizeProjectPlatforms)) {
    const idIndex = projects.findIndex((item) => item.id === project.id);
    const pathIndex = projects.findIndex(
      (item) => canonicalPath(item.path) === canonicalPath(project.path),
    );
    if (idIndex >= 0 && pathIndex >= 0 && idIndex !== pathIndex) {
      throw new Error(
        `项目「${project.name}」的 ID 与目录分别对应不同的现有项目，请先调整配置再合并。`,
      );
    }
    const index = idIndex >= 0 ? idIndex : pathIndex;
    if (index >= 0) projects[index] = { ...structuredClone(project), id: projects[index].id };
    else projects.push(structuredClone(project));
  }
  return { schemaVersion: 1, projects, settings: structuredClone(current.settings) };
}
