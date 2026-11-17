on loginShellCommand(commandText, shellPath)
 if shellPath is "" then set shellPath to "/bin/zsh"
 -- 登录交互 shell 同时加载 zprofile/zshrc 等用户环境；命令结束后保留交互 shell。
 return quoted form of shellPath & " -l -i -c " & quoted form of (commandText & linefeed & "exec " & quoted form of shellPath & " -l")
end loginShellCommand

-- Ghostty 使用原生 Apple Events 四字符代码，避免其他终端用户编译脚本时必须安装 Ghostty。
on openGhostty(terminalPath, workingDirectory, commandText)
 set launchCommand to my loginShellCommand(commandText, system attribute "SHELL")
 try
  tell application terminalPath
   set surfaceConfig to «event GhstNSCf» given «class GScS»:{«class GScD»:workingDirectory, «class GScC»:launchCommand, «class GScW»:true}
   if (count of «class Gwnd») > 0 then
    «event GhstNTab» given «class GNtW»:front «class Gwnd», «class GNtS»:surfaceConfig
   else
    «event GhstNWin» given «class GNwS»:surfaceConfig
   end if
   activate
  end tell
 on error errText number errNumber
  if errNumber is -1743 then error "请在系统设置 → 隐私与安全性 → 自动化中允许 DevDock 控制 Ghostty。" number errNumber
  error "Ghostty 原生接口调用失败，请使用支持 AppleScript 的新版 Ghostty，并确认 macos-applescript 未设为 false；未自动重试，以免重复执行命令。原始错误：" & errText number errNumber
 end try
end openGhostty

-- 事件代码来自 iTerm2 官方 iTerm2.sdef；不依赖本机安装 iTerm2 才能编译脚本。
on openITerm(terminalPath, commandText)
 set launchCommand to my loginShellCommand(commandText, system attribute "SHELL")
 try
  tell application terminalPath
   if (count of windows) > 0 then
    «event Itrmntwn» front window given «class Nwcm»:launchCommand
   else
    «event Itrmnwwn» given «class Nwcm»:launchCommand
   end if
   activate
  end tell
 on error errText number errNumber
  if errNumber is -1743 then error "请在系统设置 → 隐私与安全性 → 自动化中允许 DevDock 控制 iTerm2。" number errNumber
  error "iTerm2 原生接口调用失败；请确认使用支持 AppleScript 的 iTerm2。未自动重试，以免重复执行命令。原始错误：" & errText number errNumber
 end try
end openITerm

-- OSC 0 设置终端标签标题，兼容 iTerm 与 Ghostty。
on oscTitleCommand(theTitle)
 return "printf '\\033]0;%s\\007' " & quoted form of theTitle & " && "
end oscTitleCommand

-- 每个参数单独引用，不把用户目录或命令拼成未经引用的外层 shell 代码。
on quoteArguments(values)
 set resultText to ""
 repeat with value in values
  set resultText to resultText & quoted form of (contents of value as text) & " "
 end repeat
 return resultText
end quoteArguments

on cliArguments(terminalPath, workingDirectory, launchCommand, windowTitle)
 if terminalPath ends with "/WezTerm.app" then
  set args to {"start", "--always-new-process", "--cwd", workingDirectory, "--"}
 else if terminalPath ends with "/kitty.app" then
  set args to {"--directory", workingDirectory}
  if windowTitle is not "" then set args to args & {"--title", windowTitle}
 else if terminalPath ends with "/Alacritty.app" then
  set args to {"--working-directory", workingDirectory}
  if windowTitle is not "" then set args to args & {"--title", windowTitle}
  -- Alacritty 的 -e 必须位于选项末尾，后续参数全部属于启动程序。
  set args to args & {"-e"}
 else
  error "此终端尚未接入原生命令行接口，请选择支持的终端。"
 end if
 return args & {"/bin/sh", "-c", launchCommand}
end cliArguments

on openCliTerminal(terminalPath, workingDirectory, commandText, windowTitle)
 set launchCommand to "exec " & my loginShellCommand(commandText, system attribute "SHELL")
 set args to my cliArguments(terminalPath, workingDirectory, launchCommand, windowTitle)
 -- Launch Services 启动独立窗口，避免终端继承任务进程组后被 DevDock 的清理流程终止。
 do shell script "/usr/bin/open -n -a " & quoted form of terminalPath & " --args " & my quoteArguments(args)
end openCliTerminal

-- 授权命令作为参数传入，工作目录通过 shell 安全引用。
on run argv
 set terminalPath to item 1 of argv
 set workingDirectory to item 2 of argv
 set userCommand to item 3 of argv
 set windowTitle to ""
 if (count of argv) >= 4 then set windowTitle to item 4 of argv
 -- 未选择 AI 工具时只切换目录，不拼接空命令。
 set commandText to "cd -- " & quoted form of workingDirectory
 if userCommand is not "" then set commandText to commandText & " && " & userCommand
 if terminalPath is "Terminal" or terminalPath ends with "/Terminal.app" then
  tell application "Terminal"
   activate
   set newTab to do script commandText
   -- Terminal 的自定义标题不会被 shell 提示符覆盖。
   if windowTitle is not "" then
    try
     set custom title of newTab to windowTitle
    end try
   end if
  end tell
 else if terminalPath ends with "/Ghostty.app" then
  if windowTitle is not "" then set commandText to (my oscTitleCommand(windowTitle)) & commandText
  my openGhostty(terminalPath, workingDirectory, commandText)
 else if terminalPath ends with "/iTerm.app" or terminalPath ends with "/iTerm2.app" then
  if windowTitle is not "" then set commandText to (my oscTitleCommand(windowTitle)) & commandText
  my openITerm(terminalPath, commandText)
 else if terminalPath ends with "/WezTerm.app" or terminalPath ends with "/kitty.app" or terminalPath ends with "/Alacritty.app" then
  if windowTitle is not "" then set commandText to (my oscTitleCommand(windowTitle)) & commandText
  my openCliTerminal(terminalPath, workingDirectory, commandText, windowTitle)
 else
  error "暂不支持此终端；请选择 Terminal、Ghostty、iTerm2、WezTerm、Kitty 或 Alacritty。"
 end if
 return "终端命令已触发，退出状态由终端显示"
end run
