-- 只验证分块纯函数，不打开终端、不发送命令。
on run argv
 set automation to load script (POSIX file (item 1 of argv))
 if (automation's chunkText("", 400)) is not {} then error "空命令不应产生分块"
 if (automation's chunkText("cd -- /tmp", 400)) is not {"cd -- /tmp"} then error "短命令应保持一块"
 set longText to ""
 repeat with index from 1 to 250
  set longText to longText & "abcdefgh"
 end repeat
 set pieces to automation's chunkText(longText, 400)
 if (count of pieces) is not 5 then error "2000 字符应切成 5 块"
 set joined to ""
 repeat with piece in pieces
  if (count of characters of (contents of piece)) > 400 then error "分块超过上限"
  set joined to joined & (contents of piece)
 end repeat
 if joined is not longText then error "分块拼接必须与原文完全一致"
 set mixedText to "变量='值;含引号' 和 Emoji 😀 以及 \\\" 转义"
 set mixedPieces to automation's chunkText(mixedText, 7)
 set mixedJoined to ""
 repeat with piece in mixedPieces
  set mixedJoined to mixedJoined & (contents of piece)
 end repeat
 if mixedJoined is not mixedText then error "多字节字符分块后必须保持原文"
 return "分块粘贴（4 项）测试通过"
end run
