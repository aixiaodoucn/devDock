<script setup lang="ts">
import { computed, ref } from 'vue';
import { X, Plus, Trash2, FolderGit2, RefreshCw } from 'lucide-vue-next';
import type { Project, Tools, Action } from '../types';
import { PLATFORM_LABELS } from '../types';
import { api, desktop } from '../api';
import PathInput from './PathInput.vue';
const props = defineProps<{
  project: Project;
  tools: Tools;
  hbuilderxPath: string;
  existing: Project[];
  saving: boolean;
  detecting: boolean;
}>();
const emit = defineEmits<{ close: []; save: [project: Project]; detect: [] }>();
const draft = ref<Project>(JSON.parse(JSON.stringify(props.project)));
const tab = ref('general');
const error = ref('');
const menus = ref<string[][]>([]);
const loadingMenus = ref(false);
const reading = ref(false);
const readingScripts = ref(false);
const packageScripts = ref<{ name: string; command: string }[]>([]);
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
    platform: draft.value.platforms[0] || 'pc',
    kind: 'shell',
    menuPath: [],
    command: script.command,
  });
}
const manifestAppId = ref('');
const availablePlatforms = computed(() =>
  draft.value.type === 'uniapp'
    ? ['app', 'weixin', 'dingtalk']
    : draft.value.type === 'nodejs'
      ? ['pc', 'weixin']
      : [],
);
function changeType() {
  tab.value = 'general';
  error.value = '';
  draft.value.platforms =
    draft.value.type === 'uniapp' ? ['app'] : draft.value.type === 'nodejs' ? ['pc'] : [];
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
    platform: draft.value.platforms[0] || 'pc',
    kind: draft.value.type === 'uniapp' ? 'menu' : 'shell',
    menuPath: [],
    command: '',
  });
}
function applyPreset(kind: string) {
  const action: Action = {
    id: crypto.randomUUID(),
    name: '',
    platform: 'dingtalk',
    kind: 'menu',
    menuPath: [],
    command: '',
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
      platform: 'weixin',
      menuPath: ['运行', '运行到小程序模拟器', '微信开发者工具'],
    });
  if (kind === 'weixin-publish')
    Object.assign(action, {
      name: '微信 · 发行',
      platform: 'weixin',
      menuPath: ['发行', '小程序-微信'],
    });
  if (!draft.value.platforms.includes(action.platform)) draft.value.platforms.push(action.platform);
  draft.value.commands.push(action);
}
async function loadMenus() {
  loadingMenus.value = true;
  try {
    menus.value = await api.menus(props.hbuilderxPath);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loadingMenus.value = false;
  }
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
          <label v-if="draft.type === 'uniapp'"
            >manifest.json 路径（可选）<PathInput
              v-model="draft.manifestPath"
              :directory="false"
              placeholder="留空时自动查找根目录 / src/manifest.json"
              @error="error = $event"
          /></label>
          <label v-if="draft.type !== 'backend'"
            >项目终端（可选）<select v-model="draft.terminal">
              <option value="">跟随全局设置</option>
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
                <option value="">跟随全局 JDK / 系统 JAVA_HOME</option>
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
            >产物归档目录（可选）<PathInput
              v-model="draft.outputDir"
              placeholder="默认：uniapp 项目/unpackage/release/ai-dev"
              @error="error = $event"
          /></label>
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
            <label v-if="action.kind === 'shell'"
              >命令<textarea
                v-model="action.command"
                rows="2"
                placeholder="pnpm run dev"
                spellcheck="false"
              />
            </label>
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
                v-if="menus.length"
                aria-label="选择 HBuilderX 菜单"
                @change="action.menuPath = JSON.parse(($event.target as HTMLSelectElement).value)"
              >
                <option value="[]">从当前 HBuilderX 菜单选择…</option>
                <option v-for="path in menus" :key="path.join('/')" :value="JSON.stringify(path)">
                  {{ path.join(' / ') }}
                </option>
              </select></template
            >
          </div>
          <button
            v-if="draft.type === 'uniapp'"
            type="button"
            class="button"
            :disabled="loadingMenus || !desktop"
            @click="loadMenus"
          >
            <RefreshCw :size="15" :class="{ spinning: loadingMenus }" />读取当前 HBuilderX 菜单
          </button>
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
