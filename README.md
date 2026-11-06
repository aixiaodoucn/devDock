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
pnpm tauri build                 # 构建本机架构的 macOS 应用与 DMG
```

macOS 构建结果位于 `src-tauri/target/release/bundle/`。对外分发前需自行配置 Apple Developer 签名与公证。Windows 原生安装包需要在 Windows 构建和验证；首版外部工具自动化仅实现 macOS，其他系统会返回未支持说明。

## 配置项目

左侧添加项目，选择本地目录、类型与支持的平台：

- **uni-app**：App、微信小程序、钉钉小程序。
- **Node.js**：PC / Web、微信小程序。
- **backend**：仅保存项目记录，运行能力待后续实现。

编辑、移除仅影响工作台配置；移除不会删除本地源码。可以从 `package.json` 读取 scripts，选择添加为终端操作。命令、项目目录和菜单路径均保存在本机。

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
- JDK 按项目配置 → 全局配置 → JAVA_HOME → 系统检测兜底，仅设置构建子进程环境。
- Android 模块首版支持单层模块目录，例如 `simpleDemo`；Gradle task 可填 `:simpleDemo:assembleRelease`。Gradle 版本需是唯一的字面量声明；复杂 flavor/变量表达式会明确拒绝，不猜测修改。
- 原生资源更新前备份到 Android 工程的 `.dev-dock-resource-backups`；uni-app 源资源不会被移走。
- 产物复制到可配置的归档目录，默认 `<uniapp项目>/unpackage/release/dev-dock`。名称带版本和任务 ID，不覆盖旧包。

`scripts/build-app.sh` 和 `scripts/build.sh` 从指定的 build-app skill 脚本复制后适配，原始脚本保持不变。执行只依赖本项目随包脚本；项目路径、AppID 和 JDK 均通过配置传入。

## HBuilderX 菜单自动化

在「运行与发行」中添加操作，选择 HBuilderX 菜单方式，用 `/` 分隔层级，例如：

```text
运行 / 钉钉小程序-开发
发行 / 自定义发行 / 钉钉小程序-生产
运行 / 运行到小程序模拟器 / 微信开发者工具
发行 / 小程序-微信
```

可以读取当前 HBuilderX 的菜单树再选择。微信预设会匹配目标项目名称，排除「指定页面」菜单。自定义钉钉菜单必须已由该 uni-app 项目配置并出现在 HBuilderX 中。

执行前打开目标项目的 `App.vue`，校验前台窗口的项目上下文和菜单可用状态。窗口标题需能识别项目目录名与 App.vue；不同 HBuilderX 版本或自定义显示名称可能需要调整适配。菜单任务串行执行，避免跨项目抢占前台。

macOS 需在「系统设置 → 隐私与安全性」中为 DevDock 授予辅助功能，并按系统提示允许控制 System Events、HBuilderX 或终端。WGT 确认框必须能通过辅助功能识别，具备 WGT 标识及对应按钮；无法识别时明确失败，不盲按回车。

菜单操作只报告「已触发」，不将点击菜单误报为编译/发行成功。APK/WGT 另行检查本次新产物、版本/AppID（本地资源）、ZIP 完整性和相关进程退出状态。

## 终端

支持检测 Terminal、Ghostty、iTerm 以及手动设置应用路径。项目终端可以覆盖全局选择。

- Terminal 通过 AppleScript 打开终端执行。
- Ghostty / iTerm 使用前台自动化创建标签页并粘贴命令；需要默认的新标签页快捷键和辅助功能权限。
- 工作目录安全引用，用户自定义命令按原意执行。终端任务只显示「已触发」，运行日志与服务停止在对应终端中处理。
- 取消 DevDock 构建可以停止它管理的子进程，无法保证撤回 HBuilderX 已开始的内部发行或终端已收到的命令；请在对应工具中确认。

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
