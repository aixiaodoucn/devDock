#!/bin/bash
# Copied from ~/.agents/skills/build-app/scripts/build.sh, then adapted.
# 资源复制和 Gradle 仅用于 APK，WGT 构建不依赖它们。
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { printf '%s\n' '仅支持 macOS 构建' >&2; exit 1; }
[[ $# == 6 ]] || { printf '%s\n' '用法: build.sh RESOURCES ANDROID_ROOT MODULE JDK TASK APPID' >&2; exit 2; }
resources=$1; android_root=$2; module=$3; jdk=$4; task=$5; appid=$6
[[ -d "$resources" && -f "$android_root/gradlew" && -x "$jdk/bin/java" ]] || { printf '%s\n' '资源、Gradle wrapper 或 JDK 无效' >&2; exit 1; }
[[ "$module" =~ ^[A-Za-z0-9_-]+$ && "$task" =~ ^[A-Za-z0-9_:.-]+$ && "$task" != -* && "$appid" =~ ^[A-Za-z0-9_-]+$ ]] || exit 2
assets="$android_root/$module/src/main/assets/apps/$appid"
backup_root="$android_root/.dev-dock-resource-backups"
mkdir -p "$(dirname -- "$assets")" "$backup_root"
staging=$(mktemp -d "$backup_root/staging.XXXXXX")
# 保留资源源目录与旧原生资源，通过新目录避免旧文件混入安装包。
/usr/bin/ditto "$resources" "$staging/resources"
if [[ -e "$assets" ]]; then mv "$assets" "$staging/previous-$appid"; fi
mv "$staging/resources" "$assets"
export JAVA_HOME="$jdk"
export PATH="$JAVA_HOME/bin:$PATH"
cd -- "$android_root"
# 版本已在 Rust 层写回模块源码，不在 AGP 配置结束后重复修改 DSL。
# 禁用持久守护进程，取消任务时由 Rust 终止整个子进程组。
if [[ "$task" == :* ]]; then gradle_task=$task; else gradle_task=":$module:$task"; fi
/bin/bash ./gradlew "$gradle_task" --no-daemon --rerun-tasks
