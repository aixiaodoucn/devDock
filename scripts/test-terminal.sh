#!/bin/bash
# 验证标题、原生 shell 和 CLI 参数；仅运行测试替身，不打开终端。
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
compiled_script=$(mktemp "${TMPDIR:-/tmp/}dev-dock-terminal-test.XXXXXX")
trap 'rm -f "$compiled_script"' EXIT
osacompile -o "$compiled_script" "$script_dir/terminal.applescript"
osascript "$script_dir/test-terminal.applescript" "$compiled_script"
