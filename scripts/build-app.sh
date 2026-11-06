#!/bin/bash
# Copied from ~/.agents/skills/build-app/scripts/build-app.sh, then adapted.
# Rust 层负责版本修改、AppID 持久化恢复、取消和产物新鲜度校验。
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { printf '%s\n' '仅支持 macOS 自动化' >&2; exit 1; }
[[ $# -ge 4 ]] || { printf '%s\n' 'usage: build-app.sh MODE HBUILDER_APP PROJECT APP_VUE [MENU ...]' >&2; exit 2; }
mode=$1; hbuilder=$2; project=$3; app_vue=$4; shift 4
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
[[ -d "$hbuilder" && -f "$app_vue" ]] || { printf '%s\n' 'HBuilderX 或 App.vue 路径不存在' >&2; exit 1; }
/usr/bin/open -a "$hbuilder" "$app_vue"
sleep 2
case "$mode" in
  apk) /usr/bin/osascript "$script_dir/hbuilderx.applescript" click "$project" "$(basename -- "$project")" '发行' 'App-Android/iOS-本地打包' '生成本地打包App资源' ;;
  wgt-android|wgt-ios)
    /usr/bin/osascript "$script_dir/hbuilderx.applescript" click "$project" "$(basename -- "$project")" '发行' 'App-制作应用wgt包'
    /usr/bin/osascript "$script_dir/hbuilderx.applescript" confirm-wgt "$project" "$(basename -- "$project")"
    ;;
  menu) /usr/bin/osascript "$script_dir/hbuilderx.applescript" click "$project" "$(basename -- "$project")" "$@" ;;
  *) printf '%s\n' '未知构建模式' >&2; exit 2 ;;
esac
