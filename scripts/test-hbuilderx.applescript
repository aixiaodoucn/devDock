-- 加载已编译的脚本，只测试纯匹配函数，不点击菜单或触发构建。
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
 return "WGT 按钮名称与项目路径匹配测试通过（17 项）"
end run
