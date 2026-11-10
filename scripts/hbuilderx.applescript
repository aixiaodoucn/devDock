-- 外部参数统一通过 argv 传入，禁止拼接进 AppleScript 源码。
on isMenuPlaceholder(labelText)
 if labelText is missing value then return true
 return labelText is in {"", "数据加载中...", "数据加载中…", "加载中...", "加载中…", "Loading..."}
end isMenuPlaceholder

-- 运行中的自定义菜单会被 HBuilderX 改为“停止 + 原名称”。
on runningMenuCandidates(labelText)
 if labelText is missing value or labelText is "" then return {}
 if labelText starts with "停止" then return {}
 return {"停止" & labelText, "停止 " & labelText}
end runningMenuCandidates

on baseRunMenuName(labelText)
 if labelText is missing value then return missing value
 if labelText starts with "停止 " and (count of labelText) > 3 then return text 4 thru -1 of labelText
 if labelText starts with "停止" and (count of labelText) > 2 then return text 3 thru -1 of labelText
 return labelText
end baseRunMenuName

on findRunningMenu(theMenu, labelText)
 local stopNames, candidateLabel
 set stopNames to my runningMenuCandidates(labelText)
 tell application "System Events"
  repeat with candidateReference in stopNames
   set candidateLabel to contents of candidateReference
   if exists menu item candidateLabel of theMenu then return candidateLabel
  end repeat
 end tell
 return missing value
end findRunningMenu

on isProjectWeixinMenu(labelText, projectName)
 local baseLabel
 set baseLabel to my baseRunMenuName(labelText)
 if baseLabel is missing value then return false
 return baseLabel is ("微信开发者工具 - [" & projectName & "]") or baseLabel is ("微信开发者工具 - " & projectName)
end isProjectWeixinMenu

-- 预设微信名称在实际菜单中带项目后缀；显式停止沿用同一别名规则。
on resolveMenuLabel(theMenu, requestedLabel, projectName)
 local baseLabel, matchingNames, currentEntries, candidateName, resolvedName, runningLabel
 set baseLabel to my baseRunMenuName(requestedLabel)
 if baseLabel is not "微信开发者工具" then return requestedLabel
 tell application "System Events"
  if exists menu item requestedLabel of theMenu then return requestedLabel
  set matchingNames to {}
  set currentEntries to get menu items of theMenu
  repeat with candidateReference in currentEntries
   try
    set candidateName to name of (contents of candidateReference)
    set resolvedName to my baseRunMenuName(candidateName)
    if my isProjectWeixinMenu(resolvedName, projectName) then
     if resolvedName is not in matchingNames then set end of matchingNames to resolvedName
    end if
   on error detailText number errorNumber
    if errorNumber is not in {-1728, -1719, -10000} then error detailText number errorNumber
   end try
  end repeat
  if (count of matchingNames) is 0 then return requestedLabel
  if (count of matchingNames) is not 1 then error "微信开发者工具项目菜单不唯一，未选择指定页面菜单"
  set baseLabel to item 1 of matchingNames
  if requestedLabel starts with "停止" then
   set runningLabel to my findRunningMenu(theMenu, baseLabel)
   if runningLabel is not missing value then return runningLabel
   return "停止" & baseLabel
  end if
 end tell
 return baseLabel
end resolveMenuLabel

-- 归属检查只读，不激活窗口，不改变当前选择，更不点击运行/停止。
on inspectRunningMenu(menuArgs, projectName)
 local topName, currentMenu, labelText, runningLabel
 set topName to item 1 of menuArgs
 if topName is not "运行" then return "idle"
 tell application "System Events" to tell process "HBuilderX"
  set currentMenu to menu 1 of menu bar item topName of menu bar 1
  repeat with idx from 2 to count of menuArgs
   set labelText to my resolveMenuLabel(currentMenu, item idx of menuArgs, projectName)
   if idx is (count of menuArgs) then
    set runningLabel to my findRunningMenu(currentMenu, my baseRunMenuName(labelText))
    if runningLabel is not missing value then return "__DEVDOCK_HX_ALREADY_RUNNING__:" & my baseRunMenuName(runningLabel)
    return "idle"
   end if
   if not (exists menu item labelText of currentMenu) then error "菜单不存在: " & labelText
   set currentMenu to menu 1 of menu item labelText of currentMenu
  end repeat
 end tell
 return "idle"
end inspectRunningMenu

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
on baseButtonName(labelText)
 set baseName to labelText as text
 if (count of characters of baseName) > 3 then
  set openingMark to character -3 of baseName
  set shortcutKey to character -2 of baseName
  set closingMark to character -1 of baseName
  if ((openingMark is "(" and closingMark is ")") or (openingMark is "（" and closingMark is "）")) and "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz" contains shortcutKey then
   set baseName to text 1 thru -4 of baseName
  end if
 end if
 return baseName
end baseButtonName

on isWgtConfirmName(labelText)
 if labelText is missing value then return false
 return (my baseButtonName(labelText)) is in {"确定", "生成", "制作"}
end isWgtConfirmName

-- 发行弹框标题或说明包含发行类关键词时才允许确认，避免误按其他弹框的默认按钮。
on isReleaseDialogText(dialogText)
 if dialogText is missing value then return false
 if dialogText is "" then return false
 return (dialogText contains "发行") or (dialogText contains "发布") or (dialogText contains "打包")
end isReleaseDialogText

-- 自定义发行弹框的主按钮；只接受完整名称，避免误匹配“发行到微信”等菜单项。
on isReleaseButtonName(labelText)
 if labelText is missing value then return false
 return (my baseButtonName(labelText)) is in {"发行", "发布", "打包"}
end isReleaseButtonName

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

-- 自定义发行弹框没有项目路径可校验，只按唯一启用的发行类按钮确认。
on findReleaseConfirmation(dialogWindow)
 tell application "System Events"
  set matchedButtons to {}
  set dialogElements to entire contents of dialogWindow
  repeat with elementItem in dialogElements
   try
    if (role of elementItem) is "AXButton" then
     if my isReleaseButtonName(name of elementItem) and (enabled of elementItem) then set end of matchedButtons to contents of elementItem
    end if
   end try
  end repeat
  if (count of matchedButtons) is 1 then return item 1 of matchedButtons
  if (count of matchedButtons) > 1 then error "发行确认按钮不唯一，未点击"
 end tell
 return missing value
end findReleaseConfirmation

on run argv
 -- rows 是 System Events 的控件术语，必须使用不冲突的局部变量。
 local menuPaths, runningLabel
 set operation to item 1 of argv
 tell application "System Events"
  if not (exists process "HBuilderX") then
   if operation is "inspect-running" then return "idle"
   error "HBuilderX 未运行"
  end if
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
   if operation is "inspect-running" then
    if (count of argv) < 5 then error "菜单路径为空"
    return my inspectRunningMenu(items 4 thru -1 of argv, projectName)
   end if
   set frontmost to true
   delay 1
   if frontmost is not true then error "HBuilderX 不在前台"
   if (count of windows) = 0 then error "HBuilderX 无项目窗口"
   -- 确认弹框可能成为前台窗口，此时项目窗口校验由调用方点击菜单前完成。
   if operation is "confirm-release" or operation is "inspect-release" then
    repeat 30 times
     set candidates to (every window whose subrole is "AXDialog")
     repeat with windowItem in (every window)
      try
       if (count of sheets of windowItem) > 0 then set candidates to candidates & (sheets of windowItem)
      end try
     end repeat
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
      if my isReleaseDialogText(dialogText) then
       set targetButton to my findReleaseConfirmation(dialogWindow)
       if targetButton is not missing value then
        set buttonLabel to name of targetButton
        if operation is "inspect-release" then return "已识别发行按钮: " & buttonLabel & "；未点击"
        click targetButton
        return "发行确认对话框已触发: " & buttonLabel
       end if
       if operation is "inspect-release" then return "已识别发行弹框: " & dialogText & "；未按回车"
       -- 与手动操作一致：聚焦发行弹框后回车触发默认“发行”按钮。
       try
        perform action "AXRaise" of dialogWindow
       end try
       delay 0.3
       key code 36
       return "发行弹框已按回车确认"
      end if
     end repeat
     delay 0.5
    end repeat
    error "未发现可识别的发行确认对话框；未发送回车"
   end if
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
    set labelText to my resolveMenuLabel(currentMenu, labelText, projectName)
    if topName is "运行" and idx is (count of argv) then
     set runningLabel to my findRunningMenu(currentMenu, labelText)
     if runningLabel is not missing value then return "__DEVDOCK_HX_ALREADY_RUNNING__:" & labelText
     if labelText starts with "停止" and not (exists menu item labelText of currentMenu) then return "当前命令未运行，无需停止"
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
