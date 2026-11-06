-- 外部参数统一通过 argv 传入，禁止拼接进 AppleScript 源码。
on walkMenu(theMenu, prefix)
 set rows to {}
 tell application "System Events"
  repeat with entry in menu items of theMenu
   set labelText to name of entry
   if labelText is not missing value and labelText is not "" then
    set currentPath to prefix & {labelText}
    if (count of menus of entry) > 0 then
     set rows to rows & my walkMenu(menu 1 of entry, currentPath)
    else
     set AppleScript's text item delimiters to character id 31
     set end of rows to currentPath as text
     set AppleScript's text item delimiters to ""
    end if
   end if
  end repeat
 end tell
 return rows
end walkMenu
on run argv
 set operation to item 1 of argv
 tell application "System Events"
  if not (exists process "HBuilderX") then error "HBuilderX 未运行"
  tell process "HBuilderX"
   if operation is "list" then
    set rows to {}
    repeat with topName in {"运行", "发行"}
     set rows to rows & my walkMenu(menu 1 of menu bar item topName of menu bar 1, {contents of topName})
    end repeat
    set AppleScript's text item delimiters to linefeed
    return rows as text
   end if
   set projectPath to item 2 of argv
   set projectName to item 3 of argv
   set frontmost to true
   delay 1
   if frontmost is not true then error "HBuilderX 不在前台"
   if (count of windows) = 0 then error "HBuilderX 无项目窗口"
   set windowTitle to name of front window
   -- 同时确认项目上下文和调用方要求打开的文件。
   if operation is not "confirm-wgt" then
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
   if operation is "confirm-wgt" then
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
       repeat with buttonName in {"生成", "确定", "制作"}
        if exists button (contents of buttonName) of dialogWindow then
         set targetButton to button (contents of buttonName) of dialogWindow
         if enabled of targetButton then
          click targetButton
          return "WGT 确认对话框已确认"
         end if
        end if
       end repeat
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
