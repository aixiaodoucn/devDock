-- 外部参数统一通过 argv 传入，禁止拼接进 AppleScript 源码。
on isMenuPlaceholder(labelText)
 if labelText is missing value then return true
 return labelText is in {"", "数据加载中...", "数据加载中…", "加载中...", "加载中…", "Loading..."}
end isMenuPlaceholder

on walkMenu(theMenu, prefix)
 local menuPaths, menuEntries, menuEntry, labelText, currentPath, childMenus
 set menuPaths to {}
 tell application "System Events"
  -- 先读取列表快照，避免递归中继续使用动态 menu items 引用。
  set menuEntries to get menu items of theMenu
  repeat with entryReference in menuEntries
   try
    set menuEntry to contents of entryReference
    set labelText to name of menuEntry
    if not my isMenuPlaceholder(labelText) then
     set currentPath to prefix & {labelText}
     set childMenus to get menus of menuEntry
     if (count of childMenus) > 0 then
      set menuPaths to menuPaths & my walkMenu(item 1 of childMenus, currentPath)
     else
      set AppleScript's text item delimiters to character id 31
      set end of menuPaths to currentPath as text
      set AppleScript's text item delimiters to ""
     end if
    end if
   on error detailText number errorNumber
    -- Qt 动态菜单会替换加载占位项，失效引用不应中断其他分支；权限错误必须继续上报。
    if errorNumber is not in {-1728, -1719, -10000} then error detailText number errorNumber
   end try
  end repeat
 end tell
 return menuPaths
end walkMenu
-- Qt 在辅助功能名称中附带快捷键，例如界面显示“确定”，AX 名称为“确定(O)”。
on isWgtConfirmName(labelText)
 if labelText is missing value then return false
 set baseName to labelText as text
 if (count of characters of baseName) > 3 then
  set openingMark to character -3 of baseName
  set shortcutKey to character -2 of baseName
  set closingMark to character -1 of baseName
  if ((openingMark is "(" and closingMark is ")") or (openingMark is "（" and closingMark is "）")) and "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz" contains shortcutKey then
   set baseName to text 1 thru -4 of baseName
  end if
 end if
 return baseName is in {"确定", "生成", "制作"}
end isWgtConfirmName

on isProjectWgtPath(outputPath, projectPath)
 if outputPath is missing value then return false
 set prefixPath to projectPath as text
 if prefixPath ends with "/" then set prefixPath to text 1 thru -2 of prefixPath
 return (outputPath starts with (prefixPath & "/unpackage/release/")) and (outputPath ends with ".wgt")
end isProjectWgtPath

-- 按真实控件树寻找按钮，兼容嵌套 group；只接受同一弹框中的目标项目 WGT 路径。
on findWgtConfirmation(dialogWindow, projectPath)
 tell application "System Events"
  set matchedButtons to {}
  set matchingPath to false
  -- 先取得列表快照再遍历，避免 System Events 将 item of entire contents 当作无效引用。
  set dialogElements to entire contents of dialogWindow
  repeat with elementItem in dialogElements
   try
    set elementRole to role of elementItem
    if elementRole is "AXTextField" then
     if my isProjectWgtPath(value of elementItem as text, projectPath) then set matchingPath to true
    else if elementRole is "AXButton" then
     if my isWgtConfirmName(name of elementItem) and (enabled of elementItem) then set end of matchedButtons to contents of elementItem
    end if
   end try
  end repeat
  if matchingPath and (count of matchedButtons) is 1 then return item 1 of matchedButtons
  if matchingPath and (count of matchedButtons) > 1 then error "WGT 确认按钮不唯一，未点击"
 end tell
 return missing value
end findWgtConfirmation

on run argv
 -- rows 是 System Events 的控件术语，必须使用不冲突的局部变量。
 local menuPaths
 set operation to item 1 of argv
 tell application "System Events"
  if not (exists process "HBuilderX") then error "HBuilderX 未运行"
  tell process "HBuilderX"
   if operation is "list" then
    -- 只读扫描保持调用方在前台；执行菜单操作时才激活 HBuilderX。
    set menuPaths to {}
    repeat with topName in {"运行", "发行"}
     set menuPaths to menuPaths & my walkMenu(menu 1 of menu bar item (contents of topName) of menu bar 1, {contents of topName})
    end repeat
    if (count of menuPaths) is 0 then error "未读取到运行或发行菜单，请确认 HBuilderX 已就绪"
    set AppleScript's text item delimiters to linefeed
    return menuPaths as text
   end if
   set projectPath to item 2 of argv
   set projectName to item 3 of argv
   set frontmost to true
   delay 1
   if frontmost is not true then error "HBuilderX 不在前台"
   if (count of windows) = 0 then error "HBuilderX 无项目窗口"
   set windowTitle to name of front window
   -- 同时确认项目上下文和调用方要求打开的文件。
   if operation is not "confirm-wgt" and operation is not "inspect-wgt" then
    if windowTitle does not contain projectName or windowTitle does not contain "App.vue" then error "无法确认 HBuilderX 当前项目及 App.vue，请检查窗口标题"
    -- 若窗口提供 AXDocument，则校验完整项目路径，避免误操作同名项目。
    set documentPath to ""
    try
     set documentPath to value of attribute "AXDocument" of front window
    end try
    if documentPath is not "" and documentPath is not missing value then
     if documentPath does not contain projectPath then error "HBuilderX 文档路径与目标项目不一致"
    end if
   end if
   if operation is "confirm-wgt" or operation is "inspect-wgt" then
    repeat 20 times
     set candidates to (every window whose subrole is "AXDialog")
     if (count of sheets of front window) > 0 then set candidates to candidates & (sheets of front window)
     repeat with dialogWindow in candidates
      set dialogText to ""
      try
       set dialogText to (name of dialogWindow) as text
      end try
      try
       repeat with labelItem in (static texts of dialogWindow)
        set dialogText to dialogText & " " & (value of labelItem as text)
       end repeat
      end try
      if dialogText contains "wgt" or dialogText contains "WGT" then
       set targetButton to my findWgtConfirmation(dialogWindow, projectPath)
       if targetButton is not missing value then
        set buttonLabel to name of targetButton
        if operation is "inspect-wgt" then return "已识别 WGT 确认按钮: " & buttonLabel & "；项目路径匹配，未点击"
        click targetButton
        return "WGT 确认对话框已确认: " & buttonLabel
       end if
      end if
     end repeat
     delay 0.5
    end repeat
    error "未发现可识别的 WGT 确认对话框；未发送回车"
   end if
   if (count of argv) < 5 then error "菜单路径为空"
   set topName to item 4 of argv
   if topName is not "运行" and topName is not "发行" then error "仅允许运行或发行菜单"
   set currentMenu to menu 1 of menu bar item topName of menu bar 1
   repeat with idx from 5 to count of argv
    set labelText to item idx of argv
    -- 微信预设仅匹配目标项目的普通菜单，不匹配指定页面入口。
    if labelText is "微信开发者工具" and not (exists menu item labelText of currentMenu) then
     set matchingNames to {}
     repeat with candidateItem in menu items of currentMenu
      set candidateName to name of candidateItem
      if candidateName is not missing value then
       if candidateName is ("微信开发者工具 - [" & projectName & "]") or candidateName is ("微信开发者工具 - " & projectName) then
        if candidateName does not contain "指定页面" then set end of matchingNames to candidateName
       end if
      end if
     end repeat
     if (count of matchingNames) is not 1 then error "微信开发者工具项目菜单不唯一或不存在，未选择指定页面菜单"
     set labelText to item 1 of matchingNames
    end if
    if not (exists menu item labelText of currentMenu) then error "菜单不存在: " & labelText
    set targetItem to menu item labelText of currentMenu
    if not (enabled of targetItem) then error "菜单不可用: " & labelText
    if idx is (count of argv) then
     click targetItem
    else
     set currentMenu to menu 1 of targetItem
    end if
   end repeat
  end tell
 end tell
 return "菜单已触发"
end run
