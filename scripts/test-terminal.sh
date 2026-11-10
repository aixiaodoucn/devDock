#!/bin/bash
# 只验证长命令分块纯函数；不打开终端或执行命令。
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
compiled_script=$(mktemp "${TMPDIR:-/tmp/}dev-dock-terminal-test.XXXXXX")
trap 'rm -f "$compiled_script"' EXIT
osacompile -o "$compiled_script" "$script_dir/terminal.applescript"
osascript "$script_dir/test-terminal.applescript" "$compiled_script"
