export type ProjectType = 'uniapp' | 'nodejs' | 'backend';
export type Platform = 'app' | 'weixin' | 'dingtalk' | 'pc';
export type BuildMode = 'apk' | 'wgt-android' | 'wgt-ios';
export interface Action {
  id: string;
  name: string;
  platform: string;
  kind: 'menu' | 'shell';
  menuPath: string[];
  command: string;
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
  commands: Action[];
}
export interface Settings {
  hbuilderxPath: string;
  terminal: string;
  jdkPath: string;
}
export interface AppConfig {
  schemaVersion: 1;
  projects: Project[];
  settings: Settings;
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
  kind: 'log' | 'status' | 'artifact';
  message: string;
  status?: TaskStatus;
  path?: string;
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
  weixin: '微信小程序',
  dingtalk: '钉钉小程序',
  pc: 'PC / Web',
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
      hbuilderxPath: '/Applications/HBuilderX.app',
      terminal: '/Applications/Ghostty.app',
      jdkPath: '',
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
    commands: [],
  };
}

// 合并按 ID 和标准化路径去重，保留当前设备设置，避免导入覆盖本机工具路径。
export function mergeConfig(current: AppConfig, incoming: AppConfig): AppConfig {
  const canonicalPath = (path: string) => path.replace(/[\\/]+$/, '').replaceAll('\\', '/');
  const projects = structuredClone(current.projects);
  for (const project of incoming.projects) {
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
