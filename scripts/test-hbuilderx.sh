#!/bin/bash
# 默认只验证纯函数；--live-menus 只读检查 HBuilderX 菜单，不点击菜单或执行构建。
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
compiled_script=$(mktemp "${TMPDIR:-/tmp/}dev-dock-wgt-test.XXXXXX")
trap 'rm -f "$compiled_script"' EXIT
osacompile -o "$compiled_script" "$script_dir/hbuilderx.applescript"
osascript "$script_dir/test-hbuilderx.applescript" "$compiled_script" "$@"
