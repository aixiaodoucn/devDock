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
  nodeVersion: string;
  nvmDir: string;
  commands: Action[];
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
    description: '轻盈紫调，留一点灵感空间。',
    color: '#7760cf',
  },
  {
    id: 'blue',
    name: '深海蓝',
    english: 'OCEAN',
    description: '沉稳蓝调，让每次交付更从容。',
    color: '#3267b7',
  },
  {
    id: 'cyan',
    name: '冰川青',
    english: 'GLACIER',
    description: '清透青调，让工作空间更轻快。',
    color: '#167b89',
  },
  {
    id: 'rose',
    name: '玫瑰粉',
    english: 'ROSE',
    description: '柔和玫瑰色，温暖而不张扬。',
    color: '#ad4c70',
  },
  {
    id: 'amber',
    name: '暖砂金',
    english: 'SANDSTONE',
    description: '温润砂金色，安静沉着的质感。',
    color: '#94671f',
  },
  {
    id: 'slate',
    name: '石墨灰',
    english: 'GRAPHITE',
    description: '克制中性色，把注意力留给项目。',
    color: '#57657a',
  },
] as const;
export type ThemeName = (typeof THEME_OPTIONS)[number]['id'];
export interface Settings {
  hbuilderxPath: string;
  terminal: string;
  jdkPath: string;
  theme: ThemeName;
  openOutputAfterBuild: boolean;
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
  kind: 'log' | 'status' | 'artifact' | 'notice';
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
      theme: 'lavender',
      openOutputAfterBuild: true,
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
