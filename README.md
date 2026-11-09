# DevDock

基于 Tauri 2、Vue 3 和 Rust 的本地开发工作台。首版支持 macOS，管理 uni-app、Node.js 项目，集中执行 App 构建、HBuilderX 菜单操作和终端命令。

## 启动

需要 Node.js 22+、pnpm 11、Rust 1.89+ 和 Xcode Command Line Tools。

```sh
pnpm install
pnpm tauri dev
```

仅预览界面可运行 `pnpm dev`。浏览器预览使用独立 localStorage，不读写桌面应用配置，不执行本地构建和终端命令。

```sh
pnpm build                       # 类型检查与前端生产构建
pnpm test                        # 配置合并测试
cargo test --manifest-path src-tauri/Cargo.toml
bash scripts/test-hbuilderx.sh    # macOS：WGT 匹配和菜单占位项，不触发打包
bash scripts/test-hbuilderx.sh --live-menus # 只读检查当前 HBuilderX 菜单，不点击运行/发行
bash scripts/test-build.sh        # macOS：伪 JDK/Gradle 验证兼容参数与资源复制
pnpm tauri build                 # 构建本机架构的 macOS 应用与 DMG
```

macOS 构建结果位于 `src-tauri/target/release/bundle/`。对外分发前需自行配置 Apple Developer 签名与公证。Windows 原生安装包需要在 Windows 构建和验证；首版外部工具自动化仅实现 macOS，其他系统会返回未支持说明。

## 配置项目

左侧添加项目，选择本地目录、类型与支持的平台：

- **uni-app**：App、微信小程序、钉钉小程序。
- **Node.js**：PC / Web、微信小程序。
- **backend**：仅保存项目记录，运行能力待后续实现。

编辑、移除仅影响工作台配置；移除不会删除本地源码。可以从 `package.json` 读取 scripts，选择添加为终端操作。命令、项目目录和菜单路径均保存在本机。

## 菜单栏快捷入口

启动 DevDock 后，macOS 菜单栏显示单色图标。左键点击显示快捷弹框，再次点击、点击外部或按 Esc 收起；弹框顶部提供 Logo 和「打开 DevDock」。macOS 快捷窗由独立非激活 NSPanel 承载，允许加入当前桌面及全屏空间，普通主窗口保持原有桌面归属。

- 内容按项目分组，展开后可执行已配置的运行、发行或终端命令，支持搜索项目和命令。
- 项目右侧箭头会打开主窗口并定位对应项目；未配置快捷命令的项目可直接进入主窗口操作区。
- 快捷执行复用主窗口的权限检查、所选 nvm 版本和终端配置；任务日志可在主窗口查看。每次打开都会刷新项目配置，并跟随当前主题。向 Ghostty/iTerm 等终端交接命令前先收起快捷窗并释放键盘焦点，避免新标签或粘贴按键落入弹框。
- 关闭主窗口后应用保留在菜单栏。右键菜单栏图标可「打开 DevDock」或「退出 DevDock」；退出仍会先取消应用管理的任务并恢复 AppID。

浏览器预览 `/?view=quick` 可以查看弹框布局，但不执行本地命令；真实菜单栏交互需使用 `pnpm tauri dev` 启动的桌面开发版。

## 外观与构建偏好

- 提供「浅紫微光」「深海蓝」「冰川青」「玫瑰粉」「暖砂金」「石墨灰」六套主题，默认浅紫。右上角菜单可以直接选择，偏好设置提供全部主题预览，选择后立即保存。
- 主题保存在本机配置中，重启后保留，也随 JSON 备份导出。旧配置没有主题字段时默认浅紫，不修改原有工具路径与项目配置。
- 新图标以终端提示符组成字母 D，并加入停靠轨道；矢量源为 `public/devdock-icon.svg`，桌面 PNG/ICNS/ICO 位于 `src-tauri/icons`。界面标识与浏览器图标共用矢量源。
- 「开始构建」旁有「构建完打开输出目录」，默认勾选并保存选择。仅在 APK/WGT 构建成功、AppID 恢复完成后在访达打开输出文件夹；失败或取消时不打开。若打开目录失败，会保留成功的构建结果并显示警告。

## App 构建

在项目的「App 构建」设置中分别填写 Android AppID 和 iOS AppID。这里是 uni-app 的 `__UNI__...`，不是 Android package name 或 iOS Bundle ID。可读取当前 manifest AppID，再手动决定填入哪个平台。

支持：

| 目标 | 依赖 |
| --- | --- |
| Android APK | HBuilderX、Android 离线原生工程、Gradle wrapper、兼容的 JDK/Android SDK |
| Android WGT | HBuilderX、Android AppID |
| iOS WGT | HBuilderX、iOS AppID |

APK 采用「HBuilderX 生成本地 App 资源 → 复制资源到原生工程 → Gradle 构建」流程。工程需已完成 Android SDK、签名、DCloud 离线 SDK 等配置。DevDock 不创建或修改签名材料。

- 自动查找根目录或 `src/manifest.json`，也可手动指定。支持注释和数字/字符串 versionCode，修改时保留其他字段与注释。
- 构建面板默认读取 versionName/versionCode，用户输入的新版本写回源码。APK 同步原生模块 Gradle 版本；可选同步 `main.js` 的 `appVersion()`。
- 不自动套用 `3.4.8 → 34800` 等项目专属编码规则。versionCode 为有效的非负整数，并受原生构建版本范围约束。
- AppID 仅在构建时临时切换；成功、失败或取消后恢复原值，保留版本修改。恢复记录先落盘，崩溃后下次启动继续恢复。发现 AppID 被用户另行修改时保留冲突记录，不强行覆盖。
- JDK 下拉框右侧提供「获取本地版本」，刷新已安装列表后手动选择；刷新不覆盖当前选择，也支持手动指定路径。
- JDK 按项目配置 → 全局配置 → JAVA_HOME → 系统检测兜底，仅设置构建子进程环境；构建日志会显示实际版本和路径。
- JDK 9+ 通过构建进程的 `JAVA_TOOL_OPTIONS` 追加旧版 Android 工具所需的 `--add-opens`，同时传给 Gradle 派生 JVM，保留已有环境选项及工程 `org.gradle.jvmargs` 内存配置。JDK 8 不添加模块参数。Gradle 6.5 / AGP 4.1.1 等旧工程仍建议优先选择原工程已验证的 JDK 8；这些兼容参数不等同于升级工具链。
- Android 模块首版支持单层模块目录，例如 `simpleDemo`；Gradle task 可填 `:simpleDemo:assembleRelease`。Gradle 版本需是唯一的字面量声明；复杂 flavor/变量表达式会明确拒绝，不猜测修改。
- 原生资源更新前备份到 Android 工程的 `.dev-dock-resource-backups`；uni-app 源资源不会被移走。
- WGT 在 `<uniapp项目>/unpackage/release` 直接生成版本文件：Android 为 `3.8.8.wgt`，iOS 为 `3.8.8-ios.wgt`，不创建额外子目录。同平台同版本重新打包会替换对应文件，其他版本与平台的包保留。
- APK 复制到可配置的归档目录，默认 `<uniapp项目>/unpackage/release`，原样保留 Gradle 生成的文件名，不额外添加版本或任务 ID。例如当前原生工程输出 `hyx-版本号-yyyyMMddHHmm.apk`，归档后仍保持该名称；再次产生同名文件时，以本次完整产物替换。同目录归档不会复制覆盖源文件。历史 `dev-dock` 目录及其中产物不会自动删除。

`scripts/build-app.sh` 和 `scripts/build.sh` 从指定的 build-app skill 脚本复制后适配，原始脚本保持不变。执行只依赖本项目随包脚本；项目路径、AppID 和 JDK 均通过配置传入。

## HBuilderX 菜单自动化

在「运行与发行」中添加操作，选择 HBuilderX 菜单方式，用 `/` 分隔层级，例如：

```text
运行 / 钉钉小程序-开发
发行 / 自定义发行 / 钉钉小程序-生产
运行 / 运行到小程序模拟器 / 微信开发者工具
发行 / 小程序-微信
```

可以读取当前 HBuilderX 的菜单树再选择，只读扫描不会激活 HBuilderX 或把 DevDock 切到后台；实际执行运行/发行时才切换。微信预设会匹配目标项目名称，排除「指定页面」菜单。自定义钉钉菜单必须已由该 uni-app 项目配置并出现在 HBuilderX 中。

执行前打开目标项目的 `App.vue`，校验前台窗口的项目上下文和菜单可用状态。窗口标题需能识别项目目录名与 App.vue；不同 HBuilderX 版本或自定义显示名称可能需要调整适配。菜单任务串行执行，避免跨项目抢占前台。

用户触发 HBuilderX 构建、菜单读取/执行或 Ghostty / iTerm 操作时，DevDock 会先检测辅助功能权限；缺少权限时自动打开「系统设置 → 隐私与安全性 → 辅助功能」并暂停操作，构建尚未写入版本或切换 AppID。请手动开启 DevDock 权限，重启应用后重试。若执行过程中仍收到 `-25211`，也会打开设置并显示授权提示；设置打开失败时会提示手动前往。系统对控制 System Events、HBuilderX 或终端的自动化授权仍需按提示允许。

WGT 确认框必须能通过辅助功能识别，具备 WGT 标识、目标项目的输出路径及对应按钮。支持嵌套控件和 `确定(O)` 等 Qt 快捷键后缀；无法唯一识别时明确失败，不盲按回车。

菜单操作只报告「已触发」，不将点击菜单误报为编译/发行成功。APK/WGT 另行检查本次新产物、版本/AppID（本地资源）、ZIP 完整性和相关进程退出状态。

## 终端

支持检测 Terminal、Ghostty、iTerm 以及手动设置应用路径。项目终端可以覆盖全局选择。

- Terminal 通过 AppleScript 打开终端执行。
- Ghostty / iTerm 使用前台自动化创建标签页并粘贴命令；需要默认的新标签页快捷键和辅助功能权限。
- 工作目录安全引用，用户自定义命令按原意执行。终端任务只显示「已触发」，运行日志与服务停止在对应终端中处理。
- 取消 DevDock 构建可以停止它管理的子进程，无法保证撤回 HBuilderX 已开始的内部发行或终端已收到的命令；请在对应工具中确认。

### Node.js 版本

添加或编辑 Node.js 项目时，可以选择该项目使用的 Node.js 版本。「获取本地版本」读取本机 nvm 已安装版本，按版本号排序；也可以选择「跟随终端默认版本」。每个项目可以使用不同版本，选择随项目配置及 JSON 备份保存。

指定版本后，终端先进入项目目录，再通过 nvm 启用指定版本，然后执行所选命令。只有版本启用成功才继续运行；已卸载的版本或失效的 nvm 路径会报错，不自动安装，也不悄悄改用其他版本。切换只作用于新终端会话，不修改 nvm 默认版本或其他终端。

## JSON 备份

偏好设置提供导入/导出 JSON：

- 导出包含 `schemaVersion: 1`、完整项目配置与工具设置，不包含源码、构建产物或 JDK。
- 导入先验证版本、字段、平台、动作及项目唯一性，再选择合并或替换。
- **合并**：按 ID 或目录匹配，更新匹配项目、追加新项目，保留本机工具设置。ID 与目录分别匹配不同项目时拒绝合并，避免误覆盖。
- **替换**：以备份覆盖全部项目及工具设置。替换前可先导出当前配置。
- 导入不会执行命令。路径可能来自其他机器；执行前需确认路径和导入的自定义命令。
- 命令中的敏感值会随配置导出，请妥善保存备份文件。

macOS 配置与恢复记录位于 `~/Library/Application Support/com.devdock.desktop/`。配置写入采用临时文件与原子替换；单实例锁防止另一实例误恢复正在构建的 AppID。

首次升级会从旧版 `com.aidev.desktop` 数据目录复制配置及待恢复记录，保留旧数据，不覆盖已存在的 DevDock 配置。请先退出旧版应用再启动 DevDock，避免两版同时操作同一业务项目。应用标识已更新，macOS 可能需要重新授予自动化和辅助功能权限。旧 JSON 备份仍可导入。

## 验证边界

仓库测试覆盖配置合并/冲突、备份关系校验、JSONC 字段修改、AppID 恢复及冲突保护、版本字段、产物快照和 package scripts 读取。前端可以在浏览器独立验证。

真实 APK/WGT 发行依赖本机 HBuilderX 版本、辅助功能授权、原生工程和签名环境。语法检查、单元测试和 DevDock 自身打包成功，不代表已替任意业务项目完成真实发行。首次使用建议先用测试项目跑通 WGT，再运行 APK。
