-- 授权命令作为参数传入，工作目录通过 shell 安全引用。
on run argv
 set terminalPath to item 1 of argv
 set workingDirectory to item 2 of argv
 set userCommand to item 3 of argv
 -- 未选择 AI 工具时只切换目录，不拼接空命令。
 set commandText to "cd -- " & quoted form of workingDirectory
 if userCommand is not "" then set commandText to commandText & " && " & userCommand
 if terminalPath is "Terminal" or terminalPath ends with "/Terminal.app" then
  tell application "Terminal"
   activate
   do script commandText
  end tell
 else if terminalPath ends with "/iTerm.app" or terminalPath ends with "/iTerm2.app" or terminalPath ends with "/Ghostty.app" then
  set processName to "iTerm2"
  if terminalPath ends with "/Ghostty.app" then set processName to "Ghostty"
  do shell script "/usr/bin/open -a " & quoted form of terminalPath
  delay 1
  tell application "System Events"
   tell process processName
    set frontmost to true
    if (count of windows) = 0 then error "终端没有可用窗口，请先手动打开一个终端窗口后重试；未发送命令"
    if frontmost is not true then error "无法确认终端处于前台；未发送命令"
    keystroke "t" using command down
    delay 0.5
    if (count of windows) = 0 or frontmost is not true then error "无法确认终端新标签页上下文；未发送命令"
    -- 通过粘贴避免键盘布局破坏命令内容，完成后恢复原剪贴板。
    -- 长命令（如包含大段环境变量）粘贴耗时明显，过早回车或恢复剪贴板会截断命令。
    set pasteDelay to 0.5 + ((count of characters of commandText) / 400)
    if pasteDelay > 12 then set pasteDelay to 12
    set oldClipboard to the clipboard
    try
     set the clipboard to commandText
     keystroke "v" using command down
     delay pasteDelay
     key code 36
     delay 0.5
     set the clipboard to oldClipboard
    on error errText
     set the clipboard to oldClipboard
     error errText
    end try
   end tell
  end tell
 else
  error "暂不支持此终端；请选择 Terminal、iTerm 或 Ghostty"
 end if
 return "终端命令已触发，退出状态由终端显示"
end run
