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
 set resultText to "WGT 匹配测试通过（17 项）；菜单占位项测试通过（10 项）"
 if (count of argv) > 1 and item 2 of argv is "--live-menus" then
  tell application "System Events" to set originalFrontApp to unix id of first application process whose frontmost is true
  set menuText to run script automation with parameters {"list"}
  tell application "System Events" to set currentFrontApp to unix id of first application process whose frontmost is true
  if originalFrontApp is not currentFrontApp then error "读取菜单不应切换前台应用"
  if menuText does not contain ("运行" & character id 31) then error "未读取到运行菜单"
  if menuText does not contain ("发行" & character id 31) then error "未读取到发行菜单"
  if menuText contains "数据加载中..." then error "结果不应包含加载占位项"
  set resultText to resultText & "；实际菜单读取通过（" & (count of paragraphs of menuText) & " 条路径）"
 end if
 return resultText
end run
