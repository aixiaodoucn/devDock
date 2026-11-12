<script setup lang="ts">
import { computed, ref } from 'vue';
import { isWindows } from '../platform';
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
    text: 'uni-app 支持 App、Web、小程序，Node.js 支持 Web、小程序。微信、钉钉等统一归入小程序，具体目标由操作名称和命令区分。左侧列表可拖拽排序并保存；移除项目记录不会删除源码。',
  },
  {
    icon: Package,
    title: 'APK 与 WGT 构建',
    text: '通过 HBuilderX 菜单生成资源，结合现有 Android 原生工程离线构建 APK；支持 Android、iOS WGT，分别配置 AppID。',
  },
  {
    icon: Terminal,
    title: '终端命令与 Node 版本',
    text: '读取 package.json 脚本，macOS 使用 Ghostty、Terminal 或 iTerm，Windows 使用 Windows Terminal 或 PowerShell。可选择本地 nvm / nvm-windows 已安装版本。',
  },
  {
    icon: Terminal,
    title: '打开并执行命令',
    text: '在项目目录执行 claude、codex、git pull 等常用命令。偏好设置可设全局默认，每个项目可单独选择专用命令；也可只打开目录，不执行命令。菜单栏提供相同快捷入口。',
  },
  {
    icon: Boxes,
    title: '小程序运行与发行',
    text: '配置 HBuilderX 运行、发行菜单。识别正在运行的项目，提示命令占用，并提供同项目运行任务的停止入口。',
  },
  {
    icon: PanelLeft,
    title: '菜单栏快捷操作',
    text: '从 macOS 菜单栏或 Windows 托盘搜索项目并执行命令，快速打开项目详情；关闭主窗口后可从托盘恢复，macOS 也可点击程序坞图标恢复。',
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
      '在命令列表点击「执行」，终端会进入项目目录；若指定 Node 版本，macOS 通过 nvm 启用，Windows 仅将所选 node.exe 所在目录加入新会话 PATH，再执行命令，不改变系统版本。每条终端命令可在「运行与发行」中单独填写环境变量，如 KEY=VALUE;KEY2=VALUE2。JSON 等复杂值可用单/双引号包裹，引号内分号不会被拆分；执行该命令前导出，仅作用于本次终端。',
      '运行日志和服务停止在对应终端中处理。DevDock 的「已触发」表示命令已交给终端，不代表服务已启动成功。',
    ],
  },
  {
    title: '运行与停止小程序',
    steps: [
      '自定义菜单使用「读取 package.json uni-app.scripts」，从正在配置的项目目录读取脚本并添加运行或发行操作。每个脚本的 title 作为完整菜单名称，不按 dev/prod 名称猜用途。',
      '内置公共菜单可使用「读取 HBuilderX 菜单」。注意：读取的是 HBuilderX 当前激活项目的菜单，不一定是正在配置的项目；目标项目的自定义脚本请从 package.json 读取。',
      '两种读取结果分别展示，切换项目目录后会清除旧候选。读取文件不需要辅助功能权限或切换窗口；HBuilderX 菜单读取需要权限，但不主动切换项目。实际执行时才定位并校验目标项目。',
      '同名命令被其他项目占用时，提示实际运行项目，当前项目不会启动。停止按钮会使用原菜单名称前加「停止」的对应菜单。',
      '停止前必须能确认运行者属于当前项目；跨项目或无法确认归属时拒绝停止。自定义发行会在菜单后弹出「微信小程序发行」等确认页，DevDock 识别到发行弹框后会点击其中的发行按钮；无法识别按钮时按回车确认默认发行按钮，未检测到发行弹框则不会盲按。菜单触发后的编译、预览和发行结果请在 HBuilderX 查看。',
    ],
  },
  {
    title: '打开项目目录并执行命令',
    steps: [
      '进入「偏好设置 → 终端命令」，添加名称和执行内容，例如 Claude Code 对应 claude、拉取代码对应 git pull。快捷添加仅提供示例，不会安装工具或执行命令。',
      '在本模块选择默认终端和路径，勾选需要使用的命令。点击命令右侧「设为默认」后保存，未单独设置的项目会共用它；再次点击「默认命令」可取消，不设默认时不自动执行命令。',
      '在项目详情选择专用命令会立即保存，仅影响当前项目；选择「默认」可取消覆盖。点击「打开并执行」先进入目录再执行所选命令，未选择可用命令时提示配置。「仅打开目录」始终不执行命令。',
      '菜单栏快捷弹框中，项目折叠箭头右侧的终端图标仅打开目录，播放图标进入目录并执行项目专用或全局默认命令。未配置可用命令时只进入目录；已禁用或删除的项目专用命令不会被其他命令替代。',
      '终端使用项目配置，未指定时使用全局终端。命令使用终端默认环境，不强制启用项目为 npm 选择的 Node 版本。交接后的结果、日志和退出在终端查看。命令配置、全局默认和项目选择均随 JSON 备份保存。',
    ],
  },
  {
    title: '后端项目组与 SCM 分支',
    steps: [
      '选择后端类型并设置父目录，扫描自动过滤以点开头的文件夹；勾选后点击「保存项目」即添加。名称与目录同行显示，编辑弹框无需填写 SCM 信息，已有映射会保留；remote 和 SCM 分支在工作台只读展示。普通目录默认不选，已有配置不会因重复扫描被覆盖。',
      '在后端工作台输入 SCM HTTPS 根地址和账号密码登录，记住登录时由系统凭据存储保存会话。只填开发 SCM 项目 ID 后加载模块，会自动匹配并保存本地开发分支；测试项目可选，填写后自动填入同仓库编号的测试分支，不填写保留已有测试目标。分支为只读，无需单独保存；同步失败可重新加载模块重试。「SCM 项目与仓库」统一展示当前开发 SCM 模块和本地目录，每个关联仓库集中显示开发、测试和本地当前分支；未关联模块提供添加本地目录入口，不可勾选操作。测试独有模块不展示，其他本地配置仍可在编辑项目中维护；密码和 Cookie 不进入项目配置或备份。',
      '有效 SCM 会话会在进程内复用，同一 Cookie 不随每次进入页面或加载项目反复读写系统凭据；保存失败只提示并保留内存登录，不自动重复请求授权。首次保存或重启后恢复已记住会话时，系统仍可能要求一次授权。',
      '切换按钮需要当前开发来源已加载并保存、至少勾选一个关联仓库、开发分支和远端已填、Git 工作区正常且干净，并且没有加载/保存或执行任务；不满足时页面会说明具体原因。点击按钮先生成预检计划，确认后才切换；测试项目未填写不影响开发分支切换。',
      '勾选仓库后先预检，再核对分支、提交和是否推送，确认后执行。合并使用临时工作树，原目录保留开发分支；未提交改动、分支缺失、合并冲突或目标占用会暂停，成功结果不会被强制回滚。',
      '合并选项「pom.xml 冲突时保留测试分支版本」默认不勾选。勾选后仅对冲突的 pom.xml（含子模块）采用测试侧内容或测试侧删除；其他文件冲突仍需手工处理，无冲突的 pom 改动正常合并。预检计划会显示策略，修改选项后需重新预检。',
      '冲突可打开保留工作区手工处理，或显式中止。手工提交并确认干净后点击完成处理，不自动补推送；进入工作台会自动显示实际执行过的待处理操作，以仓库名称和中文状态区分；执行结果显示成功、失败和未执行数量及原因。未确认执行的预检记录不再显示为历史操作，恢复后不会自动继续。推送默认关闭，只有明确勾选并确认计划才执行普通测试分支推送。',
    ],
  },
  {
    title: '项目环境体检',
    steps: [
      '在项目详情点击「环境体检」，查看目录、终端、Node、环境变量语法及当前项目所需构建配置的检查结果。异常项优先处理，警告项按说明确认；不适用项不会阻止其他平台使用。',
      '体检不启动开发工具、不执行命令或修改源码。只有主动点击「请求授权」才弹出系统提示，开启权限后点击「重新体检」。可通过「编辑项目」或「偏好设置」修正配置，再重新检查。',
      '路径或安装文件存在不代表工具链完全兼容。终端默认 Node、JDK/Gradle 兼容性及实际目录写权限仍需按提示确认；Windows 暂不支持的 HBuilderX/App 构建项会标为不适用。',
    ],
  },
  {
    title: '查看构建历史',
    steps: [
      '在 uni-app 的「构建配置」标题右侧点击「构建历史」，进入当前项目的独立历史页。可查看版本、分支、耗时与成功、失败、取消或中断状态，重启后仍保留；点击「返回构建配置」可返回原先的构建目标。',
      '通过「最多显示」选择 10、20、50、100、200、500 或 1000 条记录，默认 20 条，本机会记住选择。页面展示数量不影响历史存储，减少展示条数不会删除记录或产物。',
      '点击「保留设置」可配置每项目保留 1～1000 条已结束记录，默认 100 条，对所有项目生效。降低上限需再次确认，保存后立即清理各项目最旧的超限条目，并显示清理数量；实际产物及运行中记录会保留。',
      '展开记录查看结果摘要与产物。文件存在时可在文件管理器中定位，已移动或删除的产物会标记缺失；运行中记录不能移除，确认移除其他记录只删历史条目，不删实际文件。',
      '仅记录本应用管理的 APK/WGT 构建，不将终端或菜单的「已触发」当作构建成功。历史不保存环境变量、命令或完整业务日志，当前日志仍在「任务输出」中查看。',
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
      'macOS 支持现有构建与菜单自动化；Windows 基础版提供项目管理、终端执行、环境变量、Node 选择、Git 和备份，真实 Windows 行为需在实机验收。HBuilderX 自动化与 APK/WGT 构建暂未支持 Windows。后端项目支持父目录与子仓库管理、SCM 登录及分支映射、批量切换开发分支和合并到测试分支，不启动后端业务服务。',
  },
  {
    question: '如何检查并安装 DevDock 新版本？',
    answer:
      '安装版启动时检查一次 GitHub 最新正式 Release，之后每 4 小时检查，也可点击右上角版本号手动检查。发现新版本后点击旁边的「更新」下载；下载并通过签名校验后才显示「重启更新」，保存配置后点击即可安装并重启。任务执行中需先等待完成；下载、待重启和安装期间暂停定时检查。失败可重试，退出应用会清除本次内存中的下载。浏览器预览与开发模式不执行真实更新；首次带更新功能的版本需手动安装，草稿和预发布版本不会自动提示。',
  },
  {
    question: 'iOS 可以直接生成安装包吗？',
    answer:
      '目前 iOS 仅支持生成 WGT 热更新包，不生成 IPA。Android 支持 APK 与 WGT。WGT 需要配合已有且兼容的 App 基座使用。',
  },
  {
    question: '提示辅助功能权限不足，该怎么处理？',
    answer:
      '执行需要辅助功能的操作时，DevDock 会请求 macOS 原生授权提示，并尝试打开「系统设置 → 隐私与安全性 → 辅助功能」。手动允许 DevDock 后，返回应用重新执行即可，每次重试都会重新检测权限，不要求先重启。系统是否自动将应用列入列表由 macOS 决定，未列出时仍可点击「＋」添加当前运行的 DevDock。若授权后重试仍失败，请确认已授权的应用路径与当前运行版本一致，必要时再退出重开。macOS 对 System Events、HBuilderX 或终端的自动化控制也可能单独弹出授权提示；若设置未自动打开，可手动进入上述位置。',
  },
  {
    question: '执行 npm run dev 也需要辅助功能权限吗？',
    answer:
      'npm 本身不需要。macOS 上 DevDock 使用 Ghostty / iTerm 时，要模拟新建标签、粘贴与回车，因此需要辅助功能权限；缺少权限时命令尚未交给终端。如果不希望授予此权限，可在「偏好设置 → 终端命令」将默认终端改为 Terminal 并保存，同时检查项目是否单独指定了 Ghostty / iTerm，项目终端配置会覆盖全局设置。Terminal 使用原生 AppleScript，不要求辅助功能权限，但首次可能需要允许 DevDock 自动化控制终端。Windows Terminal / PowerShell 不涉及 macOS 辅助功能权限。',
  },
  {
    question: '为什么脚本读取、HBuilderX 菜单读取或执行失败？',
    answer:
      '读取 uni-app.scripts 时，检查正在配置的项目目录中是否有合法的 package.json，脚本是否包含 title 和 env.UNI_PLATFORM。H5/Web 脚本归入 Web，微信、支付宝、钉钉、百度、抖音等 mp-* 脚本归入小程序；其他不支持的平台会注明不能导入。读取 HBuilderX 菜单需确认应用已启动及辅助功能权限，结果来自 HBuilderX 当前激活项目，不一定是正在配置的项目。实际执行时还需能识别目标项目的 App.vue 和窗口上下文，不同 HBuilderX 版本可能需要适配。',
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
      '版本列表只读取本地 nvm / nvm-windows 已安装的 Node，不会自动下载。Windows 不调用 nvm use，而为新会话前置所选 Node 的 PATH。先在本机 nvm 环境中安装所需版本，再点击获取本地版本。已卸载的版本或失效的 nvm 路径需要重新选择；也可选择跟随终端默认版本。指定版本启用失败时不会继续运行原命令。',
  },
  {
    question: '点击执行后终端没有正确运行命令怎么办？',
    answer:
      '检查项目目录、终端路径和辅助功能权限。Ghostty、iTerm 依赖默认的新标签页快捷键与粘贴操作，修改过快捷键时可能无法交接。macOS 可尝试选择 Terminal。Windows 请确认终端为 Windows Terminal 或 PowerShell，命令使用 PowerShell 语法；npm.ps1 被策略限制时可使用 npm.cmd run dev，不修改组织执行策略。具体错误在终端查看。',
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
        <span class="local-dot"></span
        >{{
          isWindows
            ? 'Windows 基础版 · HBuilderX / App 构建暂不支持'
            : 'macOS · 后端业务运行待支持 · iOS 仅支持 WGT'
        }}
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
