<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { X, Plus, Trash2, FolderGit2, RefreshCw } from 'lucide-vue-next';
import type { Project, Tools, Action, NodeVersion, UniappScript } from '../types';
import { PLATFORM_LABELS, normalizeProjectPlatforms } from '../types';
import { api, desktop } from '../api';
import PathInput from './PathInput.vue';
const props = defineProps<{
  project: Project;
  tools: Tools;
  hbuilderxPath?: string;
  existing: Project[];
  saving: boolean;
  detecting: boolean;
}>();
const emit = defineEmits<{ close: []; save: [project: Project]; detect: [] }>();
const draft = ref<Project>(normalizeProjectPlatforms(JSON.parse(JSON.stringify(props.project))));
const tab = ref('general');
const error = ref('');
const hbuilderMenus = ref<string[][]>([]);
const loadingHbuilderMenus = ref(false);
const hbuilderMenusNotice = ref('');
let hbuilderMenusRequest = 0;
const uniappScripts = ref<UniappScript[]>([]);
const loadingUniappScripts = ref(false);
const uniappScriptsNotice = ref('');
let uniappScriptsRequest = 0;
const uniappActions = computed(() =>
  uniappScripts.value.flatMap((script) => [
    {
      name: `${script.title} · 运行`,
      platform: script.platform,
      menuPath: script.runMenuPath,
      uniPlatform: script.uniPlatform,
    },
    {
      name: `${script.title} · 发行`,
      platform: script.platform,
      menuPath: script.publishMenuPath,
      uniPlatform: script.uniPlatform,
    },
  ]),
);
// 项目目录或类型变化后，旧候选和在途读取结果均不能用于新项目。
watch(
  () => [draft.value.path, draft.value.type],
  () => {
    uniappScriptsRequest++;
    hbuilderMenusRequest++;
    hbuilderMenus.value = [];
    loadingHbuilderMenus.value = false;
    hbuilderMenusNotice.value = '';
    uniappScripts.value = [];
    uniappScriptsNotice.value = '';
    loadingUniappScripts.value = false;
  },
);
const reading = ref(false);
const readingScripts = ref(false);
const packageScripts = ref<{ name: string; command: string }[]>([]);
const nodeVersions = ref<NodeVersion[]>([]);
const readingNodes = ref(false);
const nodesLoaded = ref(false);
const nodeKey = (version: Pick<NodeVersion, 'version' | 'nvmDir'>) =>
  JSON.stringify([version.nvmDir, version.version]);
const selectedNodeKey = computed(() =>
  draft.value.nodeVersion
    ? nodeKey({ version: draft.value.nodeVersion, nvmDir: draft.value.nvmDir })
    : '',
);
const selectedNode = computed(() =>
  nodeVersions.value.find((version) => nodeKey(version) === selectedNodeKey.value),
);
async function loadNodeVersions() {
  if (readingNodes.value) return;
  readingNodes.value = true;
  try {
    nodeVersions.value = await api.nodeVersions();
    nodesLoaded.value = true;
  } catch (reason) {
    error.value = String(reason);
  } finally {
    readingNodes.value = false;
  }
}
function selectNodeVersion(event: Event) {
  const key = (event.target as HTMLSelectElement).value;
  const version = nodeVersions.value.find((item) => nodeKey(item) === key);
  draft.value.nodeVersion = version?.version || '';
  draft.value.nvmDir = version?.nvmDir || '';
}
watch(
  () => draft.value.type,
  (type) => {
    if (type === 'nodejs' && desktop && !nodesLoaded.value) loadNodeVersions();
  },
  { immediate: true },
);
async function loadScripts() {
  readingScripts.value = true;
  try {
    packageScripts.value = await api.packageScripts(draft.value);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    readingScripts.value = false;
  }
}
function addScript(script: { name: string; command: string }) {
  draft.value.commands.push({
    id: crypto.randomUUID(),
    name: script.name,
    platform: draft.value.platforms[0] || 'web',
    kind: 'shell',
    menuPath: [],
    command: script.command,
    envVars: '',
  });
}
const manifestAppId = ref('');
const availablePlatforms = computed(() =>
  draft.value.type === 'uniapp'
    ? ['app', 'web', 'miniapp']
    : draft.value.type === 'nodejs'
      ? ['web', 'miniapp']
      : [],
);
function changeType() {
  tab.value = 'general';
  error.value = '';
  draft.value.platforms =
    draft.value.type === 'uniapp' ? ['app'] : draft.value.type === 'nodejs' ? ['web'] : [];
  draft.value.commands = [];
}
function togglePlatform(platform: string) {
  const platforms = draft.value.platforms;
  draft.value.platforms = platforms.includes(platform)
    ? platforms.filter((item) => item !== platform)
    : [...platforms, platform];
}
async function readManifest() {
  reading.value = true;
  error.value = '';
  try {
    manifestAppId.value = (await api.manifest(draft.value)).appid;
  } catch (reason) {
    error.value = String(reason);
  } finally {
    reading.value = false;
  }
}
function addAction() {
  draft.value.commands.push({
    id: crypto.randomUUID(),
    name: '',
    platform: draft.value.platforms[0] || 'web',
    kind: draft.value.type === 'uniapp' ? 'menu' : 'shell',
    menuPath: [],
    command: '',
    envVars: '',
  });
}
function applyPreset(kind: string) {
  const action: Action = {
    id: crypto.randomUUID(),
    name: '',
    platform: 'miniapp',
    kind: 'menu',
    menuPath: [],
    command: '',
    envVars: '',
  };
  if (kind === 'dingtalk-run')
    Object.assign(action, { name: '开发环境 · 运行', menuPath: ['运行', '钉钉小程序-开发'] });
  if (kind === 'dingtalk-publish')
    Object.assign(action, {
      name: '生产环境 · 发行',
      menuPath: ['发行', '自定义发行', '钉钉小程序-生产'],
    });
  if (kind === 'weixin-run')
    Object.assign(action, {
      name: '微信 · 运行',
      platform: 'miniapp',
      menuPath: ['运行', '运行到小程序模拟器', '微信开发者工具'],
    });
  if (kind === 'weixin-publish')
    Object.assign(action, {
      name: '微信 · 发行',
      platform: 'miniapp',
      menuPath: ['发行', '小程序-微信'],
    });
  if (!draft.value.platforms.includes(action.platform)) draft.value.platforms.push(action.platform);
  draft.value.commands.push(action);
}
async function loadHbuilderMenus() {
  if (loadingHbuilderMenus.value) return;
  const request = ++hbuilderMenusRequest;
  loadingHbuilderMenus.value = true;
  hbuilderMenus.value = [];
  hbuilderMenusNotice.value = '';
  error.value = '';
  try {
    const menus = await api.menus(props.hbuilderxPath || props.tools.hbuilderxPath);
    if (request !== hbuilderMenusRequest) return;
    const unique = new Map<string, string[]>();
    for (const source of menus) {
      const path = [...source];
      // 微信内置运行项可能带当前项目后缀，保存通用名称以便执行时定位目标项目。
      if (
        path[0] === '运行' &&
        path[1] === '运行到小程序模拟器' &&
        path.at(-1)?.includes('微信开发者工具') &&
        !path.at(-1)?.includes('指定页面')
      ) {
        path[path.length - 1] = '微信开发者工具';
      }
      unique.set(JSON.stringify(path), path);
    }
    hbuilderMenus.value = [...unique.values()];
    hbuilderMenusNotice.value = menus.length
      ? `已读取 HBuilderX 当前激活项目的菜单，共 ${hbuilderMenus.value.length} 项。`
      : 'HBuilderX 当前激活项目没有可读取的运行或发行菜单。';
  } catch (reason) {
    if (request === hbuilderMenusRequest) error.value = String(reason);
  } finally {
    if (request === hbuilderMenusRequest) loadingHbuilderMenus.value = false;
  }
}
function selectHbuilderMenu(event: Event, existing?: Action) {
  const select = event.target as HTMLSelectElement;
  const value = select.value;
  select.value = '';
  if (value === '') return;
  const path = hbuilderMenus.value[Number(value)];
  if (!path) return;
  if (
    !existing &&
    draft.value.commands.some(
      (action) =>
        action.kind === 'menu' && JSON.stringify(action.menuPath) === JSON.stringify(path),
    )
  ) {
    hbuilderMenusNotice.value = '该菜单操作已存在，未重复添加。';
    return;
  }
  const menu = path.join(' ');
  const platform = /小程序|微信|钉钉|支付宝|百度|抖音|字节|QQ|飞书|快手|小红书|京东/.test(menu)
    ? 'miniapp'
    : /浏览器|H5|网站|web/i.test(menu)
      ? 'web'
      : existing?.platform || draft.value.platforms[0] || 'app';
  if (!draft.value.platforms.includes(platform)) draft.value.platforms.push(platform);
  const action: Action = existing ?? {
    id: crypto.randomUUID(),
    name: '',
    platform,
    kind: 'menu',
    menuPath: [],
    command: '',
    envVars: '',
  };
  action.name = action.name.trim() || `${path.at(-1)} · ${path[0]}`;
  action.platform = platform;
  action.kind = 'menu';
  action.menuPath = [...path];
  action.command = '';
  if (!existing) draft.value.commands.push(action);
}
async function loadUniappScripts() {
  if (loadingUniappScripts.value) return;
  if (!draft.value.path.trim()) {
    error.value = '请先选择项目目录，再读取 package.json 中的 uni-app.scripts。';
    tab.value = 'general';
    return;
  }
  const request = ++uniappScriptsRequest;
  loadingUniappScripts.value = true;
  error.value = '';
  uniappScripts.value = [];
  uniappScriptsNotice.value = '';
  try {
    const scripts = await api.uniappScripts(JSON.parse(JSON.stringify(draft.value)));
    if (request !== uniappScriptsRequest) return;
    uniappScripts.value = scripts;
    const unsupported = [
      ...new Set(scripts.filter((script) => !script.platform).map((script) => script.uniPlatform)),
    ];
    uniappScriptsNotice.value = scripts.length
      ? `已读取 ${scripts.length} 个 uni-app 脚本，可选择添加运行或发行操作。${unsupported.length ? `暂不支持导入的平台：${unsupported.join('、')}。` : ''}`
      : '此项目的 package.json 未配置 uni-app.scripts。';
  } catch (reason) {
    if (request === uniappScriptsRequest) error.value = String(reason);
  } finally {
    if (request === uniappScriptsRequest) loadingUniappScripts.value = false;
  }
}
function selectUniappAction(event: Event, existing?: Action) {
  const select = event.target as HTMLSelectElement;
  const value = select.value;
  select.value = '';
  if (value === '') return;
  const candidate = uniappActions.value[Number(value)];
  if (!candidate?.platform) return;
  if (
    !existing &&
    draft.value.commands.some(
      (action) =>
        action.kind === 'menu' &&
        JSON.stringify(action.menuPath) === JSON.stringify(candidate.menuPath),
    )
  ) {
    uniappScriptsNotice.value = '该操作已存在，未重复添加。';
    return;
  }
  if (!draft.value.platforms.includes(candidate.platform))
    draft.value.platforms.push(candidate.platform);
  const action = existing ?? {
    id: crypto.randomUUID(),
    name: '',
    platform: '',
    kind: 'menu' as const,
    menuPath: [],
    command: '',
    envVars: '',
  };
  action.name = action.name.trim() || candidate.name;
  action.platform = candidate.platform;
  action.kind = 'menu';
  action.menuPath = [...candidate.menuPath];
  action.command = '';
  if (!existing) draft.value.commands.push(action);
}
// 与后端一致：值以引号开头时，引号内的分号和换行不拆分。
function splitEnvEntries(text: string): string[] | string {
  const entries: string[] = [];
  let current = '';
  let afterEquals = false;
  let valueStarted = false;
  let quote = '';
  let escaped = false;
  for (const ch of text) {
    if (quote) {
      current += ch;
      if (escaped) escaped = false;
      else if (quote === '"' && ch === '\\') escaped = true;
      else if (ch === quote) {
        quote = '';
        valueStarted = true;
      }
      continue;
    }
    if (ch === ';' || ch === '\n' || ch === '\r') {
      entries.push(current);
      current = '';
      afterEquals = false;
      valueStarted = false;
      escaped = false;
      continue;
    }
    current += ch;
    if (!afterEquals) {
      if (ch === '=') afterEquals = true;
    } else if (!valueStarted) {
      if (ch === '"' || ch === "'") quote = ch;
      else if (ch !== ' ' && ch !== '\t') valueStarted = true;
    }
  }
  if (quote) return '环境变量引号未闭合';
  entries.push(current);
  return entries;
}
function envVarsError(text: string) {
  const entries = splitEnvEntries(text);
  if (typeof entries === 'string') return entries;
  const seen = new Set<string>();
  for (const entry of entries) {
    const item = entry.trim();
    if (!item) continue;
    const index = item.indexOf('=');
    if (index <= 0) return `环境变量缺少名称或等号：${item}`;
    const key = item.slice(0, index).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return `环境变量名称无效：${key}`;
    if (seen.has(key)) return `环境变量重复：${key}`;
    seen.add(key);
  }
  return '';
}
function submit() {
  error.value = '';
  const item = draft.value;
  item.name = item.name.trim();
  item.path = item.path.trim().replace(/\/$/, '');
  if (!item.name || !item.path) {
    error.value = '请填写项目名称和目录。';
    tab.value = 'general';
    return;
  }
  if (!item.path.startsWith('/')) {
    error.value = '请填写完整的绝对路径。';
    tab.value = 'general';
    return;
  }
  if (
    props.existing.some(
      (other) => other.id !== item.id && other.path.replace(/\/$/, '') === item.path,
    )
  ) {
    error.value = '此目录已添加，请编辑已有项目。';
    tab.value = 'general';
    return;
  }
  // 环境变量属于单条终端命令，逐项校验并定位到运行与发行。
  for (const action of item.commands) {
    if (action.kind !== 'shell') continue;
    const envError = envVarsError(action.envVars);
    if (envError) {
      error.value = `「${action.name.trim() || '未命名命令'}」${envError}`;
      tab.value = 'actions';
      return;
    }
  }
  if (item.type !== 'backend' && !item.platforms.length) {
    error.value = '请至少选择一个平台。';
    tab.value = 'general';
    return;
  }
  if (
    item.commands.some(
      (action) =>
        !action.name.trim() ||
        !item.platforms.includes(action.platform) ||
        (action.kind === 'shell' ? !action.command.trim() : action.menuPath.length < 2),
    )
  ) {
    error.value = '请补全操作名称、已启用的平台，以及命令或至少两级菜单路径。';
    tab.value = 'actions';
    return;
  }
  emit('save', JSON.parse(JSON.stringify(item)));
}
</script>
<template>
  <div class="modal-backdrop" @click.self="!saving && emit('close')">
    <section
      class="modal project-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="project-dialog-title"
      @keydown.esc="!saving && emit('close')"
    >
      <header class="modal-header">
        <div>
          <div class="eyebrow">PROJECT CONFIGURATION</div>
          <h2 id="project-dialog-title">
            <FolderGit2 :size="21" />{{
              existing.some((item) => item.id === project.id) ? '编辑项目' : '添加项目'
            }}
          </h2>
        </div>
        <button class="icon-button" aria-label="关闭" :disabled="saving" @click="emit('close')">
          <X :size="20" />
        </button>
      </header>
      <div class="project-type-control">
        <label for="project-type">项目类型</label>
        <select id="project-type" v-model="draft.type" :disabled="saving" @change="changeType">
          <option value="uniapp">uni-app</option>
          <option value="nodejs">Node.js</option>
          <option value="backend">后端项目（预留）</option>
        </select>
      </div>
      <nav class="tabs modal-tabs" aria-label="项目设置分类">
        <button :class="{ active: tab === 'general' }" @click="tab = 'general'">基本信息</button
        ><button
          v-if="draft.type === 'uniapp' && draft.platforms.includes('app')"
          :class="{ active: tab === 'build' }"
          @click="tab = 'build'"
        >
          App 构建</button
        ><button
          v-if="draft.type !== 'backend'"
          :class="{ active: tab === 'actions' }"
          @click="tab = 'actions'"
        >
          运行与发行
        </button>
      </nav>
      <form id="project-form" class="modal-body" @submit.prevent="submit">
        <div v-if="error" class="inline-message error" role="alert">{{ error }}</div>
        <div v-show="tab === 'general'" class="form-stack">
          <label>项目名称<input v-model="draft.name" placeholder="例如：门店管理 App" /></label>
          <label
            >项目目录<PathInput
              v-model="draft.path"
              placeholder="选择本机项目文件夹"
              label="项目目录"
              @error="error = $event"
          /></label>
          <div v-if="availablePlatforms.length">
            <span class="field-label">支持平台</span>
            <div class="platform-options">
              <button
                v-for="platform in availablePlatforms"
                :key="platform"
                type="button"
                :class="['platform-option', { selected: draft.platforms.includes(platform) }]"
                :aria-pressed="draft.platforms.includes(platform)"
                @click="togglePlatform(platform)"
              >
                <span class="check-square">{{ draft.platforms.includes(platform) ? '✓' : '' }}</span
                >{{ PLATFORM_LABELS[platform] }}
              </button>
            </div>
          </div>
          <div v-if="draft.type === 'nodejs'" class="node-version-setting">
            <label for="project-node-version" class="field-label">Node.js 版本</label>
            <div class="version-select-row">
              <select
                id="project-node-version"
                :value="selectedNodeKey"
                :disabled="readingNodes"
                @change="selectNodeVersion"
              >
                <option value="">跟随终端默认版本</option>
                <option
                  v-for="version in nodeVersions"
                  :key="nodeKey(version)"
                  :value="nodeKey(version)"
                >
                  {{ version.version }}（nvm）
                </option>
                <option v-if="draft.nodeVersion && !selectedNode" :value="selectedNodeKey">
                  {{ draft.nodeVersion }}（{{ nodesLoaded ? '本机未检测到' : '已保存' }}）
                </option>
              </select>
              <button
                type="button"
                class="button"
                :disabled="readingNodes || !desktop"
                @click="loadNodeVersions"
              >
                <RefreshCw :size="14" :class="{ spinning: readingNodes }" />{{
                  readingNodes ? '获取中…' : '获取本地版本'
                }}
              </button>
            </div>
            <p v-if="selectedNode" class="field-help runtime-path">{{ selectedNode.path }}</p>
            <p v-else-if="draft.nodeVersion" class="field-help">
              已指定 {{ draft.nodeVersion }}。执行前会检查版本是否仍安装，缺少时停止运行。
            </p>
            <p v-else class="field-help">
              选择版本后，执行命令前通过 nvm 启用。仅作用于本次终端，不改变默认版本。
            </p>
            <p v-if="nodesLoaded && !nodeVersions.length" class="field-help">
              未发现已安装的 nvm Node.js 版本，可先使用终端默认版本。
            </p>
          </div>
          <label v-if="draft.type === 'uniapp'"
            >manifest.json 路径（可选）<PathInput
              v-model="draft.manifestPath"
              :directory="false"
              placeholder="留空时自动查找根目录 / src/manifest.json"
              @error="error = $event"
          /></label>
          <label v-if="draft.type !== 'backend'"
            >项目终端（可选）<select v-model="draft.terminal">
              <option value="">默认</option>
              <option v-for="tool in tools.terminals" :key="tool.path" :value="tool.path">
                {{ tool.name }}
              </option>
              <option
                v-if="draft.terminal && !tools.terminals.some((t) => t.path === draft.terminal)"
                :value="draft.terminal"
              >
                {{ draft.terminal }}
              </option>
            </select></label
          >
          <div class="note">项目只记录目录和配置。删除项目记录不会删除本地文件。</div>
        </div>
        <div v-show="tab === 'build'" class="form-stack">
          <div class="section-heading">
            <div>
              <h3>平台 AppID</h3>
              <p>构建时切换，任务结束后恢复原始值。</p>
            </div>
            <button
              type="button"
              class="button small"
              :disabled="reading || !desktop"
              @click="readManifest"
            >
              <RefreshCw :size="14" :class="{ spinning: reading }" />读取当前
            </button>
          </div>
          <div v-if="manifestAppId" class="inline-message">
            当前生效：<code>{{ manifestAppId }}</code>
            <div class="button-row">
              <button class="text-button" type="button" @click="draft.androidAppId = manifestAppId">
                填入 Android</button
              ><button class="text-button" type="button" @click="draft.iosAppId = manifestAppId">
                填入 iOS
              </button>
            </div>
          </div>
          <div class="form-grid">
            <label
              >Android AppID<input v-model="draft.androidAppId" placeholder="__UNI__..." /></label
            ><label>iOS AppID<input v-model="draft.iosAppId" placeholder="__UNI__..." /></label>
          </div>
          <hr />
          <div class="section-heading">
            <div>
              <h3>Android 离线构建</h3>
              <p>以下配置仅用于 APK，WGT 不需要原生工程和 JDK。</p>
            </div>
          </div>
          <label
            >Android 原生工程目录<PathInput
              v-model="draft.androidProjectPath"
              placeholder="包含 gradlew 的工程根目录"
              @error="error = $event"
          /></label>
          <div class="form-grid">
            <label
              >Android 模块<input v-model="draft.androidModule" placeholder="simpleDemo" /></label
            ><label
              >Gradle 任务<input
                v-model="draft.gradleTask"
                placeholder=":simpleDemo:assembleRelease"
            /></label>
          </div>
          <div>
            <label for="project-jdk" class="field-label">JDK 版本</label>
            <div class="jdk-select-row">
              <select id="project-jdk" v-model="draft.jdkPath">
                <option value="">默认 JDK / 系统 JAVA_HOME</option>
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
                class="button"
                type="button"
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
          <label
            >自定义 JDK 路径<PathInput
              v-model="draft.jdkPath"
              placeholder="可手动选择 JDK Home"
              @error="error = $event"
          /></label>
          <label
            >APK 归档目录（可选）<PathInput
              v-model="draft.outputDir"
              placeholder="默认：uniapp 项目/unpackage/release"
              @error="error = $event"
          /></label>
          <p class="field-help">
            WGT 固定输出到项目的 unpackage/release：版本号.wgt /
            版本号-ios.wgt，同平台同版本重新打包会替换原包。
          </p>
          <label class="checkbox-label"
            ><input v-model="draft.syncMainVersion" type="checkbox" />同步 main.js 中 appVersion()
            的返回值</label
          >
          <p class="field-help">
            仅在项目使用此版本函数时启用。versionName 与 versionCode 在构建面板填写。
          </p>
        </div>
        <div v-show="tab === 'actions'" class="form-stack">
          <div class="section-heading">
            <div>
              <h3>项目操作</h3>
              <p>菜单路径使用 / 分隔；Shell 命令将在外部终端执行。</p>
            </div>
            <button class="button small" type="button" @click="addAction">
              <Plus :size="15" />添加
            </button>
          </div>
          <div v-if="draft.type === 'uniapp'" class="preset-row">
            <span>快速添加</span
            ><button type="button" @click="applyPreset('dingtalk-run')">钉钉运行</button
            ><button type="button" @click="applyPreset('dingtalk-publish')">钉钉发行</button
            ><button type="button" @click="applyPreset('weixin-run')">微信运行</button
            ><button type="button" @click="applyPreset('weixin-publish')">微信发行</button>
          </div>
          <button
            type="button"
            class="button"
            :disabled="readingScripts || !desktop"
            @click="loadScripts"
          >
            <RefreshCw :size="15" :class="{ spinning: readingScripts }" />读取 package.json scripts
          </button>
          <div v-if="packageScripts.length" class="preset-row">
            <span>点击添加</span
            ><button
              v-for="script in packageScripts"
              :key="script.name"
              type="button"
              @click="addScript(script)"
            >
              {{ script.name }}
            </button>
          </div>
          <template v-if="draft.type === 'uniapp'">
            <button
              type="button"
              class="button"
              :disabled="loadingUniappScripts || !desktop"
              @click="loadUniappScripts"
            >
              <RefreshCw :size="15" :class="{ spinning: loadingUniappScripts }" />{{
                loadingUniappScripts ? '读取中…' : '读取 package.json uni-app.scripts'
              }}
            </button>
            <p v-if="uniappScriptsNotice" class="field-help" role="status">
              {{ uniappScriptsNotice }}
            </p>
            <select
              v-if="uniappActions.length"
              aria-label="从 uni-app.scripts 添加操作"
              @change="selectUniappAction($event)"
            >
              <option value="">选择运行或发行操作添加…</option>
              <option
                v-for="(candidate, candidateIndex) in uniappActions"
                :key="candidateIndex"
                :value="String(candidateIndex)"
                :disabled="!candidate.platform"
              >
                {{ candidate.menuPath.join(' → ')
                }}{{ candidate.platform ? '' : `（${candidate.uniPlatform} 暂不支持）` }}
              </option>
            </select>
          </template>
          <template v-if="draft.type === 'uniapp'">
            <button
              type="button"
              class="button"
              :disabled="loadingHbuilderMenus || !desktop"
              @click="loadHbuilderMenus"
            >
              <RefreshCw :size="15" :class="{ spinning: loadingHbuilderMenus }" />{{
                loadingHbuilderMenus ? '读取 HBuilderX 菜单中…' : '读取 HBuilderX 菜单'
              }}
            </button>
            <p class="field-help">
              读取的是 HBuilderX
              当前激活项目的菜单，不一定是正在配置的项目。用于选择内置公共菜单；目标项目的自定义脚本请通过上方
              package.json 读取。
            </p>
            <p v-if="hbuilderMenusNotice" class="field-help" role="status">
              {{ hbuilderMenusNotice }}
            </p>
            <select
              v-if="hbuilderMenus.length"
              aria-label="从 HBuilderX 当前激活项目菜单添加操作"
              @change="selectHbuilderMenu($event)"
            >
              <option value="">从 HBuilderX 当前激活项目菜单选择…</option>
              <option
                v-for="(path, menuIndex) in hbuilderMenus"
                :key="menuIndex"
                :value="String(menuIndex)"
              >
                {{ path.join(' → ') }}
              </option>
            </select>
          </template>
          <div v-if="!draft.commands.length" class="empty-inline">
            尚未配置操作。添加常用命令，让下一次启动更简单。
          </div>
          <div v-for="(action, index) in draft.commands" :key="action.id" class="action-editor">
            <div class="action-editor-header">
              <span class="mono">{{ String(index + 1).padStart(2, '0') }}</span
              ><button
                class="icon-button danger-text"
                type="button"
                aria-label="删除操作"
                @click="draft.commands.splice(index, 1)"
              >
                <Trash2 :size="15" />
              </button>
            </div>
            <div class="form-grid">
              <label
                >操作名称<input v-model="action.name" placeholder="例如：开发环境 · 运行" /></label
              ><label
                >所属平台<select v-model="action.platform">
                  <option v-for="platform in draft.platforms" :key="platform" :value="platform">
                    {{ PLATFORM_LABELS[platform] }}
                  </option>
                </select></label
              >
            </div>
            <label
              >执行方式<select v-model="action.kind">
                <option value="shell">终端命令</option>
                <option v-if="draft.type === 'uniapp'" value="menu">HBuilderX 菜单</option>
              </select></label
            >
            <template v-if="action.kind === 'shell'">
              <label
                >命令<textarea
                  v-model="action.command"
                  rows="2"
                  placeholder="pnpm run dev"
                  spellcheck="false"
                />
              </label>
              <label
                >环境变量（可选）<textarea
                  v-model="action.envVars"
                  rows="2"
                  spellcheck="false"
                  :aria-label="`命令 ${index + 1} 的环境变量`"
                  placeholder="KEY=VALUE;KEY2=VALUE2"
                />
              </label>
              <div class="env-hint">
                <p class="field-help env-help">
                  仅作用于当前命令：用分号或换行分隔，值可包含空格和符号，执行前导出到本次终端。
                </p>
                <p class="env-example">
                  <span>普通值</span
                  ><code>ALERT_EMAIL_SMTP_HOST=smtp.qq.com;ALERT_EMAIL_SMTP_PORT=465</code>
                </p>
                <p class="env-example">
                  <span>JSON 数组</span
                  ><code>AI_CONFIG='[{"name":"weixin","model":"DeepSeek"}]'</code>
                </p>
              </div>
            </template>
            <template v-else
              ><label
                >菜单路径<input
                  :value="action.menuPath.join(' / ')"
                  placeholder="运行 / 钉钉小程序-开发"
                  @change="
                    action.menuPath = ($event.target as HTMLInputElement).value
                      .split('/')
                      .map((p) => p.trim())
                      .filter(Boolean)
                  " /></label
              ><select
                v-if="uniappActions.length"
                aria-label="从 uni-app.scripts 选择菜单"
                @change="selectUniappAction($event, action)"
              >
                <option value="">从 uni-app.scripts 选择菜单…</option>
                <option
                  v-for="(candidate, candidateIndex) in uniappActions"
                  :key="candidateIndex"
                  :value="String(candidateIndex)"
                  :disabled="!candidate.platform"
                >
                  {{ candidate.menuPath.join(' → ')
                  }}{{ candidate.platform ? '' : `（${candidate.uniPlatform} 暂不支持）` }}
                </option>
              </select>
              <select
                v-if="hbuilderMenus.length"
                aria-label="从 HBuilderX 当前激活项目菜单选择"
                @change="selectHbuilderMenu($event, action)"
              >
                <option value="">从 HBuilderX 当前激活项目菜单选择…</option>
                <option
                  v-for="(path, menuIndex) in hbuilderMenus"
                  :key="menuIndex"
                  :value="String(menuIndex)"
                >
                  {{ path.join(' → ') }}
                </option>
              </select>
            </template>
          </div>
        </div>
      </form>
      <footer class="modal-footer">
        <span>配置仅保存在本机</span>
        <div class="button-row">
          <button class="button" :disabled="saving" @click="emit('close')">取消</button
          ><button type="submit" form="project-form" class="button primary" :disabled="saving">
            {{ saving ? '保存中…' : '保存项目' }}
          </button>
        </div>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.env-hint {
  display: grid;
  gap: 5px;
}
.env-hint .env-help {
  margin: 0;
}
.env-example {
  display: flex;
  align-items: baseline;
  gap: 7px;
  margin: 0;
  min-width: 0;
  font-size: 10px;
  color: var(--muted);
}
.env-example span {
  flex-shrink: 0;
  color: var(--text-secondary);
}
.env-example code {
  min-width: 0;
  overflow-wrap: anywhere;
  font-family: var(--mono);
  font-size: 10px;
  background: var(--accent-soft);
  border-radius: 4px;
  padding: 2px 5px;
}
</style>
