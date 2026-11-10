<script setup lang="ts">
import { computed, ref } from 'vue';
import {
  BookOpen,
  Boxes,
  CircleHelp,
  Search,
  ChevronDown,
  Package,
  Terminal,
  Layers,
  Palette,
  Archive,
  PanelLeft,
} from 'lucide-vue-next';

const tab = ref<'features' | 'guide' | 'faq'>('features');
const query = ref('');
const tabs = [
  { id: 'features', label: '功能概览', icon: Boxes },
  { id: 'guide', label: '使用说明', icon: BookOpen },
  { id: 'faq', label: '常见问题', icon: CircleHelp },
] as const;
const features = [
  {
    icon: Layers,
    title: '集中管理本地项目',
    text: '添加、编辑和移除 uni-app、Node.js 与后端项目记录，按项目配置平台、路径和常用命令。移除记录不会删除源码。',
  },
  {
    icon: Package,
    title: 'APK 与 WGT 构建',
    text: '通过 HBuilderX 菜单生成资源，结合现有 Android 原生工程离线构建 APK；支持 Android、iOS WGT，分别配置 AppID。',
  },
  {
    icon: Terminal,
    title: '终端命令与 Node 版本',
    text: '读取 package.json 脚本，使用 Ghostty、Terminal 或 iTerm 执行。Node.js 项目可选择本地 nvm 已安装版本。',
  },
  {
    icon: Terminal,
    title: '在终端中打开项目',
    text: '从任意项目详情打开项目终端，可选启动 Claude Code、Codex、CodeBuddy 等 AI 工具；不选择工具时仅进入项目目录。AI 工具与启动命令在偏好设置中维护。',
  },
  {
    icon: Boxes,
    title: '小程序运行与发行',
    text: '配置 HBuilderX 运行、发行菜单。识别正在运行的项目，提示命令占用，并提供同项目运行任务的停止入口。',
  },
  {
    icon: PanelLeft,
    title: '菜单栏快捷操作',
    text: '从 macOS 菜单栏搜索项目并执行命令，快速打开项目详情；关闭主窗口后仍可使用，点击程序坞图标可恢复主窗口。',
  },
  {
    icon: Archive,
    title: '配置备份与恢复',
    text: '导出完整 JSON 配置，导入时选择合并或替换。备份包含项目和工具设置，不包含源码、构建产物或开发环境。',
  },
  {
    icon: Palette,
    title: '外观与工作区',
    text: '六种主题、可拖拽并记忆宽度的侧栏。项目目录下显示当前 Git 分支，分支信息只读；长项目名悬停可查看完整名称。',
  },
];
const guides = [
  {
    title: '首次使用',
    steps: [
      '进入「偏好设置」，检测并确认 HBuilderX、终端和 JDK 路径。项目中填写的终端或 JDK 会优先于全局设置。',
      '点击左侧「添加项目」，选择项目类型和本地文件夹，再勾选需要使用的平台。',
      '按项目类型完成配置，保存后在项目详情中构建或执行命令。首次控制外部应用时，按系统提示授予权限。',
    ],
  },
  {
    title: '构建 Android APK / WGT',
    steps: [
      '在 uni-app 项目的「App 构建」配置中，确认 manifest 路径，分别填写 Android、iOS 的 uni-app AppID。',
      '构建 APK 还需配置已可独立构建的 Android 离线原生工程、模块、Gradle task 和兼容 JDK。旧工程优先使用其已验证的 JDK 版本。',
      '在项目详情选择构建目标，检查版本名称与版本号，点击「开始构建」。版本会写回源码，构建临时使用目标 AppID，结束后恢复原 AppID。',
      'WGT 保存在项目的 unpackage/release：Android 为「版本号.wgt」，iOS 为「版本号-ios.wgt」。APK 归档目录可配置，文件名保留 Gradle 原名。',
      '成功后可自动打开输出目录；点击日志下方的产物可在访达中定位。失败时先检查任务日志，取消后请同时确认 HBuilderX 内部操作的状态。',
    ],
  },
  {
    title: '运行 Node.js 项目',
    steps: [
      '添加 Node.js 项目，选择平台；使用「获取本地版本」检测 nvm 已安装版本，或选择「跟随终端默认版本」。',
      '读取 package.json 脚本，选择添加为命令，也可以手动填写。按需为项目选择专用终端。',
      '在命令列表点击「执行」，终端会进入项目目录；若指定 Node 版本，先通过 nvm 启用该版本，再执行命令。',
      '运行日志和服务停止在对应终端中处理。DevDock 的「已触发」表示命令已交给终端，不代表服务已启动成功。',
    ],
  },
  {
    title: '运行与停止小程序',
    steps: [
      '在 uni-app 项目的运行与发行设置中，添加 HBuilderX 菜单操作。可先「读取当前 HBuilderX 菜单」，再选择对应层级。',
      '在项目详情或菜单栏快捷弹框执行。实际执行会切换到 HBuilderX，并核对目标项目上下文；只读菜单扫描不会抢焦点。',
      '同名命令被其他项目占用时，提示实际运行项目，当前项目不会启动。停止按钮会使用原菜单名称前加「停止」的对应菜单。',
      '停止前必须能确认运行者属于当前项目；跨项目或无法确认归属时拒绝停止。菜单触发后的编译、预览和发行结果请在 HBuilderX 查看。',
    ],
  },
  {
    title: '在终端中打开项目与 AI 工具',
    steps: [
      '进入「偏好设置 → 开发工具 → AI 工具」，快捷添加常用工具，或自定义名称和启动命令。快捷添加只提供命令示例，请先确认本机已安装且能在终端正常启动。',
      '勾选需要使用的工具，点击「保存设置」。命令可包含参数或带引号的可执行文件路径；安装和登录在对应工具中完成。',
      '返回任意项目详情，在「在终端中打开」中选择 AI 工具并点击「打开」，先进入目录再启动工具。也可点击「仅打开目录」，不执行 AI 命令且不改变常用工具选择。工具选择会在本机记忆，未选择工具时「打开」也只进入目录。使用项目终端，未配置时使用默认终端。',
      'AI 工具使用终端默认环境，不强制启用项目为 npm 选择的 Node 版本。交接后的日志、会话和退出在终端中处理，DevDock 不确认 AI 会话是否启动成功。',
    ],
  },
  {
    title: '备份与迁移配置',
    steps: [
      '进入「偏好设置」导出 JSON，保存项目配置和全局工具设置。命令里填写的敏感值也会包含在备份中，请妥善保管。',
      '导入时，「合并」更新匹配项目并追加新项目，保留本机工具设置；「替换」覆盖全部项目和工具设置，建议先导出当前配置。',
      '迁移到其他机器后检查目录、终端、JDK 和 nvm 版本，准备好对应的本地开发环境，再执行命令。',
    ],
  },
];
const faqs = [
  {
    question: '支持哪些系统？后端项目可以运行吗？',
    answer:
      '当前外部工具自动化优先支持 macOS。Windows 自动化尚未实现；后端项目支持管理记录、打开项目终端及可选的 AI 工具，后端业务运行和发布流程待后续支持。',
  },
  {
    question: 'iOS 可以直接生成安装包吗？',
    answer:
      '目前 iOS 仅支持生成 WGT 热更新包，不生成 IPA。Android 支持 APK 与 WGT。WGT 需要配合已有且兼容的 App 基座使用。',
  },
  {
    question: '提示辅助功能权限不足，该怎么处理？',
    answer:
      '应用会尝试打开「系统设置 → 隐私与安全性 → 辅助功能」。手动允许 DevDock，按提示重启后重试。macOS 对 System Events、HBuilderX 或终端的自动化控制也可能单独弹出授权提示。若设置未自动打开，可手动进入上述位置。',
  },
  {
    question: '为什么 HBuilderX 菜单读取或执行失败？',
    answer:
      '先确认 HBuilderX 已启动、目标 uni-app 项目已打开，并且对应运行或发行菜单确实存在。自定义钉钉菜单依赖项目配置。检查辅助功能权限；菜单变化后重新读取。执行时需要能识别目标项目的 App.vue 和窗口上下文，不同 HBuilderX 版本可能需要适配。',
  },
  {
    question: '为什么提示其他项目已经在运行，或不允许停止？',
    answer:
      'HBuilderX 的同名命令可能共用运行槽位。DevDock 会读取实时编译进程判断实际项目，避免将当前编辑的项目误认成运行者。跨项目占用时不会替你停止；无法可靠确认归属时也会拒绝停止，请到 HBuilderX 中确认。',
  },
  {
    question: '“已触发”是不是代表构建或启动成功？',
    answer:
      '终端和 HBuilderX 菜单操作的「已触发」只表示命令交接或菜单触发完成，实际结果需要在对应工具中查看。DevDock 管理的 APK/WGT 构建会另外检查产物，再报告成功或失败。',
  },
  {
    question: 'Node 版本列表为空，或指定版本无法启用怎么办？',
    answer:
      '版本列表只读取本地 nvm 已安装的 Node，不会自动下载。先在本机 nvm 环境中安装所需版本，再点击获取本地版本。已卸载的版本或失效的 nvm 路径需要重新选择；也可选择跟随终端默认版本。指定版本启用失败时不会继续运行原命令。',
  },
  {
    question: '点击执行后终端没有正确运行命令怎么办？',
    answer:
      '检查项目目录、终端路径和辅助功能权限。Ghostty、iTerm 依赖默认的新标签页快捷键与粘贴操作，修改过快捷键时可能无法交接。可尝试选择 Terminal，再到终端中查看实际报错。菜单栏快捷弹框会先收起，再把命令交给终端。',
  },
  {
    question: 'APK 构建报 JDK 或 Gradle 错误怎么办？',
    answer:
      '先查看日志里的实际 JDK 版本与路径，确认它和原生工程的 Gradle、Android Gradle Plugin 兼容。旧工程常需要 JDK 8；项目 JDK 优先于全局设置。兼容参数不能替代工具链匹配，还需确认 SDK、签名和离线原生工程自身能够正常构建。',
  },
  {
    question: '版本和 AppID 会修改我的源码吗？',
    answer:
      '你填写的新版本会写回源码；APK 还会同步原生模块版本，可选同步 main.js。AppID 只在构建时临时切换，成功、失败或取消后恢复。若恢复前 AppID 被另外修改，应用保留冲突记录，不强行覆盖。异常退出后下次启动会尝试恢复，请关注恢复提示。',
  },
  {
    question: '取消任务会停止所有外部操作吗？',
    answer:
      '取消可以停止 DevDock 管理的构建子进程并恢复 AppID，但无法保证撤回 HBuilderX 已开始的内部操作或终端已收到的命令。终端服务请在终端停止，HBuilderX 内部任务请在 HBuilderX 中确认。',
  },
  {
    question: '关闭窗口后如何再次打开？如何退出应用？',
    answer:
      '关闭主窗口会将其隐藏，应用保留在菜单栏。点击程序坞图标，或点击菜单栏图标后选择「打开 DevDock」，都可恢复主窗口。右键菜单栏图标选择「退出 DevDock」才会退出；退出前会等待受管理任务取消和 AppID 恢复。',
  },
  {
    question: '为什么看不到 Git 分支？可以切换分支吗？',
    answer:
      '分支信息显示在项目目录下方，进入项目或窗口重新获得焦点时刷新。非仓库显示「非 Git 仓库」，detached HEAD 显示「游离 HEAD」。读取失败可悬停查看原因，检查 Git 是否安装和目录是否可访问。此功能只读，不提供分支切换。',
  },
  {
    question: '浏览器预览为什么不能构建或执行命令？',
    answer:
      '浏览器预览仅用于查看界面，使用独立的预览数据，不读取桌面项目配置，也不能控制本地工具。构建、文件选择、终端和菜单自动化需要在 DevDock 桌面应用中使用。',
  },
];
const filteredFaqs = computed(() => {
  const needle = query.value.trim().toLowerCase();
  return faqs.filter((faq) => `${faq.question} ${faq.answer}`.toLowerCase().includes(needle));
});
</script>

<template>
  <section class="help-page">
    <div class="page-heading">
      <div>
        <div class="eyebrow">DEVDock / HELP</div>
        <h1>使用帮助<span class="heading-dot">.</span></h1>
        <p class="help-intro">了解能做什么，找到使用方法和常见问题的答案。</p>
      </div>
      <BookOpen class="help-heading-icon" :size="32" aria-hidden="true" />
    </div>
    <nav class="tabs help-tabs" aria-label="帮助分类">
      <button
        v-for="item in tabs"
        :key="item.id"
        :class="{ active: tab === item.id }"
        :aria-current="tab === item.id ? 'page' : undefined"
        @click="tab = item.id"
      >
        <component :is="item.icon" :size="15" />{{ item.label }}
      </button>
    </nav>
    <div v-if="tab === 'features'">
      <div class="help-scope">
        <span class="local-dot"></span>当前优先支持 macOS · 后端业务运行待支持 · iOS 仅支持 WGT
      </div>
      <div class="help-features">
        <article v-for="feature in features" :key="feature.title" class="panel help-feature">
          <component :is="feature.icon" :size="19" class="help-feature-icon" aria-hidden="true" />
          <h2>{{ feature.title }}</h2>
          <p>{{ feature.text }}</p>
        </article>
      </div>
    </div>
    <div v-else-if="tab === 'guide'" class="help-guides">
      <article v-for="(guide, index) in guides" :key="guide.title" class="panel help-guide">
        <h2>
          <span class="help-step-number">0{{ index + 1 }}</span
          >{{ guide.title }}
        </h2>
        <ol>
          <li v-for="step in guide.steps" :key="step">{{ step }}</li>
        </ol>
      </article>
    </div>
    <div v-else class="help-faqs">
      <label class="help-search"
        ><Search :size="16" /><input
          v-model="query"
          type="search"
          aria-label="搜索常见问题"
          placeholder="搜索问题，例如权限、Node、WGT…"
      /></label>
      <p class="help-result-count">{{ filteredFaqs.length }} 个问题</p>
      <details v-for="faq in filteredFaqs" :key="faq.question" class="help-faq">
        <summary>{{ faq.question }}<ChevronDown :size="15" /></summary>
        <p>{{ faq.answer }}</p>
      </details>
      <p v-if="!filteredFaqs.length" class="panel help-no-results">
        没有匹配的问题，试试其他关键词。
      </p>
    </div>
  </section>
</template>

<style scoped>
.help-page {
  max-width: 1100px;
  margin: auto;
  padding: 20px 24px 28px;
}
.help-intro {
  color: var(--text-secondary);
  font-size: 12px;
  margin-top: 8px;
  line-height: 1.6;
}
.help-heading-icon,
.help-feature-icon {
  color: var(--accent-text);
  flex-shrink: 0;
}
.help-tabs {
  margin-bottom: 18px;
}
.help-scope {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-bottom: 14px;
  color: var(--text-secondary);
  font-size: 11px;
  line-height: 1.7;
}
.help-features {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.help-feature {
  padding: 18px;
}
.help-page h2 {
  font-size: 14px;
  line-height: 1.5;
}
.help-feature h2 {
  margin: 10px 0 7px;
}
.help-feature p,
.help-faq p,
.help-guide li {
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.9;
  overflow-wrap: anywhere;
}
.help-guides {
  display: grid;
  gap: 12px;
}
.help-guide {
  padding: 18px 20px;
}
.help-guide h2 {
  display: flex;
  align-items: center;
  gap: 10px;
}
.help-step-number {
  color: var(--accent-text);
  font-family: var(--mono);
  font-size: 11px;
}
.help-guide ol {
  margin: 12px 0 0;
  padding-left: 22px;
}
.help-guide li + li {
  margin-top: 7px;
}
.help-search {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  color: var(--muted);
  border: 1px solid var(--line);
  background: var(--surface);
  border-radius: 7px;
  padding: 0 12px;
}
.help-search > svg {
  flex-shrink: 0;
}
.help-search input {
  flex: 1;
  min-width: 0;
  padding: 10px 0;
  border: 0;
  background: none;
  box-shadow: none;
}
.help-result-count {
  color: var(--muted);
  font-size: 11px;
  margin: 12px 0;
}
.help-faq {
  border-bottom: 1px solid var(--line);
}
.help-faq summary {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 15px 0;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  list-style: none;
}
.help-faq summary::-webkit-details-marker {
  display: none;
}
.help-faq summary svg {
  flex-shrink: 0;
  color: var(--muted);
}
.help-faq[open] summary {
  color: var(--accent-text);
}
.help-faq[open] summary svg {
  transform: rotate(180deg);
}
.help-faq p {
  padding: 0 24px 16px 0;
}
.help-no-results {
  color: var(--muted);
  font-size: 12px;
  padding: 24px;
  text-align: center;
}
@media (max-width: 900px) {
  .help-page {
    padding: 20px 20px 24px;
  }
  .help-features {
    grid-template-columns: 1fr;
  }
}
</style>
