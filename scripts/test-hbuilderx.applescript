-- 默认只测试纯函数；--live-menus 可只读验证当前 HBuilderX 菜单，不点击或触发构建。
on run argv
 set automation to load script (POSIX file (item 1 of argv))
 repeat with labelText in {"确定", "确定(O)", "确定（O）", "生成(G)", "制作(M)"}
  if not (automation's isWgtConfirmName(contents of labelText)) then error "应当识别确认按钮: " & labelText
 end repeat
 repeat with labelText in {"取消(C)", "浏览", "不确定(O)", "确定(其它)", "确定()", "确定(1)"}
  if automation's isWgtConfirmName(contents of labelText) then error "不应识别此按钮: " & labelText
 end repeat
 if automation's isWgtConfirmName(missing value) then error "不应识别空名称"
 repeat with labelText in {"发行", "发行(E)", "发行（E）", "发布", "打包(P)"}
  if not (automation's isReleaseButtonName(contents of labelText)) then error "应当识别发行确认按钮: " & labelText
 end repeat
 repeat with labelText in {"取消(C)", "确定(O)", "发行到微信", "发行并上传", "发布页面", "运行", "发行(Other)", "发行()"}
  if automation's isReleaseButtonName(contents of labelText) then error "不应识别发行确认按钮: " & labelText
 end repeat
 if automation's isReleaseButtonName(missing value) then error "发行按钮不应识别空名称"
 repeat with dialogText in {"微信小程序发行", "HBuilderX - 发行", "自定义发行", "App 打包"}
  if not (automation's isReleaseDialogText(contents of dialogText)) then error "应识别发行弹框文本: " & dialogText
 end repeat
 repeat with dialogText in {"", "运行", "选择运行平台", "设置", missing value}
  if automation's isReleaseDialogText(contents of dialogText) then error "不应把其他弹框当作发行弹框: " & dialogText
 end repeat
 if not (automation's isProjectWgtPath("/项目/门店/unpackage/release/__UNI__A.wgt", "/项目/门店")) then error "应识别目标项目路径"
 if not (automation's isProjectWgtPath("/项目/门店/unpackage/release/__UNI__A.wgt", "/项目/门店/")) then error "应支持项目路径末尾斜杠"
 repeat with outputPath in {"/项目/门店2/unpackage/release/__UNI__A.wgt", "/项目/门店/unpackage/release/old.apk", "/另一个项目/unpackage/release/__UNI__A.wgt"}
  if automation's isProjectWgtPath(contents of outputPath, "/项目/门店") then error "不应接受其他项目或格式: " & outputPath
 end repeat
 repeat with labelText in {"", "数据加载中...", "数据加载中…", "加载中...", "加载中…", "Loading..."}
  if not (automation's isMenuPlaceholder(contents of labelText)) then error "应跳过加载占位菜单"
 end repeat
 if not (automation's isMenuPlaceholder(missing value)) then error "应跳过无名称菜单"
 repeat with labelText in {"运行", "发行", "钉钉小程序-开发"}
  if automation's isMenuPlaceholder(contents of labelText) then error "不应跳过正常菜单: " & labelText
 end repeat
 if (automation's runningMenuCandidates("钉钉小程序-开发")) is not {"停止钉钉小程序-开发", "停止 钉钉小程序-开发"} then error "运行状态应匹配同一命令的停止入口"
 if (automation's runningMenuCandidates("停止钉钉小程序-开发")) is not {} then error "明确配置的停止命令不应被当作重复启动"
 if (automation's runningMenuCandidates("")) is not {} then error "空菜单不应生成运行状态候选"
 if (automation's baseRunMenuName("停止微信开发者工具 - [shop]")) is not "微信开发者工具 - [shop]" then error "应识别运行中的微信菜单别名"
 if (automation's baseRunMenuName("停止 钉钉小程序-开发")) is not "钉钉小程序-开发" then error "应兼容停止前缀后的空格"
 if (automation's baseRunMenuName("发行到微信")) is not "发行到微信" then error "不应修改普通菜单名"
 if (automation's runningMenuCandidates(missing value)) is not {} then error "缺失菜单不应生成候选"
 if (automation's runningMenuCandidates("停止 微信开发者工具")) is not {} then error "显式停止预设不能识别为启动"
 if (automation's baseRunMenuName(missing value)) is not missing value then error "缺失名称应保持缺失"
 if (automation's baseRunMenuName("停止微信开发者工具")) is not "微信开发者工具" then error "停止微信应保留预设基名"
 if (automation's baseRunMenuName("停止;$(touch never)")) is not ";$(touch never)" then error "命令文本只能作为名称处理"
 if (automation's runningMenuCandidates("项目 ' ;$(touch never)")) is not {"停止项目 ' ;$(touch never)", "停止 项目 ' ;$(touch never)"} then error "菜单特殊字符必须原样保留"
 repeat with labelText in {"微信开发者工具 - [shop]", "微信开发者工具 - shop", "停止微信开发者工具 - [shop]", "停止 微信开发者工具 - shop"}
  if not (automation's isProjectWeixinMenu(contents of labelText, "shop")) then error "应匹配目标项目的运行/停止微信别名"
 end repeat
 repeat with labelText in {"微信开发者工具 - [other]", "停止微信开发者工具 - [shop] 指定页面", "微信开发者工具"}
  if automation's isProjectWeixinMenu(contents of labelText, "shop") then error "不能匹配其他项目或指定页面微信菜单"
 end repeat
 set resultText to "WGT 匹配（17 项）、菜单占位项（10 项）、重复运行及停止命令（12 项）、微信别名（7 项）、发行弹框识别（19 项）测试通过"
 if (count of argv) > 1 and item 2 of argv is "--live-menus" then
  tell application "System Events" to set originalFrontApp to unix id of first application process whose frontmost is true
  set menuText to run script automation with parameters {"list"}
  set idleState to run script automation with parameters {"inspect-running", "/fake-read-only-project", "", "运行", "停止__DEVDOCK_TEST_DOES_NOT_EXIST__"}
  if idleState is not "idle" then error "不存在的运行命令应提示无需停止"
  set inspectedCount to 0
  repeat with menuLine in paragraphs of menuText
   set AppleScript's text item delimiters to character id 31
   set menuArgs to text items of (contents of menuLine)
   set AppleScript's text item delimiters to ""
   if item 1 of menuArgs is "运行" and (last item of menuArgs) starts with "停止" then
    set stateText to run script automation with parameters ({"inspect-running", "/fake-read-only-project", ""} & menuArgs)
    if stateText does not start with "__DEVDOCK_HX_ALREADY_RUNNING__:" then error "未识别实际停止菜单，且检查不应点击"
    set inspectedCount to inspectedCount + 1
   end if
  end repeat
  tell application "System Events" to set currentFrontApp to unix id of first application process whose frontmost is true
  if originalFrontApp is not currentFrontApp then error "读取菜单不应切换前台应用"
  if menuText does not contain ("运行" & character id 31) then error "未读取到运行菜单"
  if menuText does not contain ("发行" & character id 31) then error "未读取到发行菜单"
  if menuText contains "数据加载中..." then error "结果不应包含加载占位项"
  set resultText to resultText & "；实际菜单读取通过（" & (count of paragraphs of menuText) & " 条路径，" & inspectedCount & " 个停止菜单只读检查）"
 end if
 return resultText
end run
