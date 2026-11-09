#!/bin/bash
# 编译并验证纯匹配函数，不操作 HBuilderX，也不执行业务项目构建。
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
compiled_script=$(mktemp "${TMPDIR:-/tmp/}dev-dock-wgt-test.XXXXXX")
trap 'rm -f "$compiled_script"' EXIT
osacompile -o "$compiled_script" "$script_dir/hbuilderx.applescript"
osascript "$script_dir/test-hbuilderx.applescript" "$compiled_script"
