-- 验证标题和原生终端 shell 参数；不打开终端或执行用户命令。
on run argv
 set automation to load script (POSIX file (item 1 of argv))
 set titleCommand to automation's oscTitleCommand("项目 · 开发")
 if titleCommand does not contain "]0;%s" then error "OSC 标题命令必须包含标题占位符"
 if titleCommand does not contain "'项目 · 开发'" then error "标题必须使用单引号安全引用"
 if titleCommand does not start with "printf '" then error "OSC 标题命令格式不正确"
 set quotedTitle to automation's oscTitleCommand("a'b")
 if quotedTitle does not contain "'a'\\''b'" then error "标题中的单引号必须正确转义"
 -- 用临时 shell 替身验证多层引用后的实际参数，不启动终端或用户命令。
 set tempDir to do shell script "mktemp -d"
 set fakeShell to tempDir & "/测试 shell"
 set fakeSource to "#!/bin/sh" & linefeed & "printf '%s\\n' \"$@\"" & linefeed
 do shell script "printf '%s' " & quoted form of fakeSource & " > " & quoted form of fakeShell & " && chmod +x " & quoted form of fakeShell
 set originalCommand to "printf '%s' \"中文 $HOME `literal`\"; echo 'a'\\''b'"
 repeat 300 times
  set originalCommand to originalCommand & " 中文😀'\\\"$;"
 end repeat
 set projectOptions to automation's ghosttyProjectConfig(tempDir, originalCommand, "/bin/zsh")
 if «class GScC» of projectOptions is not "'/bin/zsh' -l -i" then error "项目必须启动持续存在的交互 shell"
 if «class GScE» of projectOptions is not {"DEVDOCK_INITIAL_ACTION=" & originalCommand} then error "项目命令未完整保留"
 if «class GScI» of projectOptions is not (" eval \"unset DEVDOCK_INITIAL_ACTION; $DEVDOCK_INITIAL_ACTION\"" & return) then error "初始输入只能包含固定交接语句"
 set wrappedCommand to automation's loginShellCommand(originalCommand, fakeShell)
 set actualArgs to do shell script wrappedCommand without altering line endings
 set expectedArgs to "-l" & linefeed & "-i" & linefeed & "-c" & linefeed & originalCommand & linefeed & "exec " & quoted form of fakeShell & " -l" & linefeed
 if actualArgs is not expectedArgs then error "原生终端 shell 参数必须完整保留原命令。实际：" & actualArgs & "；预期：" & expectedArgs
 if (automation's loginShellCommand("", "")) does not start with "'/bin/zsh' -l -i -c " then error "缺少 SHELL 时应使用 zsh"
 do shell script "rm -f " & quoted form of fakeShell & "; rmdir " & quoted form of tempDir
 -- CLI 参数按独立字符串交接，目录、标题和命令中的元字符不能被外层 shell 执行。
 set testDirectory to "/tmp/中文 路径' $HOME `literal`"
 set testTitle to "项目 '标题' $(literal)"
 set testCommand to "exec '/bin/zsh' -l -i -c 'printf hello'"
 repeat with bundle in {"/Applications/WezTerm.app", "/Applications/kitty.app", "/Applications/Alacritty.app", "/Applications/Tabby.app"}
  set cliArgs to automation's cliArguments(contents of bundle, testDirectory, testCommand, testTitle)
  if contents of bundle is not "/Applications/Tabby.app" and cliArgs does not contain testDirectory then error "CLI 缺少完整目录参数"
  if last item of cliArgs is not testCommand then error "CLI 命令必须完整传入"
  set actualArgs to do shell script ("printf '%s\\n' " & automation's quoteArguments(cliArgs)) without altering line endings
  set expectedArgs to ""
  repeat with value in cliArgs
   set expectedArgs to expectedArgs & contents of value & linefeed
  end repeat
  if actualArgs is not expectedArgs then error "CLI 参数被外层 shell 改写"
 end repeat
 set wezArgs to automation's cliArguments("/Applications/WezTerm.app", testDirectory, testCommand, "")
 if items 1 thru 5 of wezArgs is not {"start", "--always-new-process", "--cwd", testDirectory, "--"} then error "WezTerm 必须创建独立进程并分隔程序参数"
 set kittyArgs to automation's cliArguments("/Applications/kitty.app", testDirectory, testCommand, "")
 if items 1 thru 2 of kittyArgs is not {"--directory", testDirectory} then error "Kitty 必须指定初始目录"
 if kittyArgs contains "--title" then error "未指定标题时应保留 Kitty 默认标题"
 set alacrittyArgs to automation's cliArguments("/Applications/Alacritty.app", testDirectory, testCommand, "")
 if items 1 thru 4 of alacrittyArgs is not {"--working-directory", testDirectory, "-e", "/bin/sh"} then error "Alacritty 的 -e 后必须直接跟启动程序"
 set tabbyArgs to automation's cliArguments("/Applications/Tabby.app", testDirectory, testCommand, "")
 if tabbyArgs is not {"run", "--", "/bin/sh", "-c", testCommand} then error "Tabby 必须使用原生 run 并用 -- 保留 shell 选项"
 set unknownRejected to false
 try
  automation's cliArguments("/Applications/Unknown.app", testDirectory, testCommand, testTitle)
 on error
  set unknownRejected to true
 end try
 if not unknownRejected then error "不支持的终端必须拒绝执行"
 return "终端标题、原生 shell、CLI 参数测试通过"
end run
