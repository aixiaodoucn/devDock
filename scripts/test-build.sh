#!/bin/bash
# 使用临时资源、伪 JDK 和伪 Gradle 验证启动参数，不改动真实 Android 工程。
set -euo pipefail
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
fixture_root=$(mktemp -d "${TMPDIR:-/tmp/}dev-dock-gradle-test.XXXXXX")
trap 'rm -r -- "$fixture_root"' EXIT
for major in 8 17; do
  test_root="$fixture_root/JDK $major"
  resources="$test_root/resources"
  android_root="$test_root/android project"
  jdk="$test_root/jdk home"
  mkdir -p "$resources/www" "$jdk/bin" "$android_root/simpleDemo/src/main/assets/apps/__UNI__TEST"
  printf '%s\n' 'new resource' > "$resources/www/index.html"
  printf '%s\n' 'previous resource' > "$android_root/simpleDemo/src/main/assets/apps/__UNI__TEST/previous.txt"
  if [[ "$major" == 8 ]]; then version=1.8.0_462; else version=17.0.18; fi
  printf '%s\n' '#!/bin/bash' "printf '%s\\n' 'openjdk version \"$version\"' >&2" > "$jdk/bin/java"
  chmod +x "$jdk/bin/java"
  printf '%s\n' '#!/bin/bash' 'printf "JAVA_HOME=%s\nJAVA_TOOL_OPTIONS=%s\n" "$JAVA_HOME" "${JAVA_TOOL_OPTIONS:-}" > "$TEST_CAPTURE"' 'printf "ARG=%s\n" "$@" >> "$TEST_CAPTURE"' > "$android_root/gradlew"
  captured_file="$test_root/captured.txt"
  JAVA_TOOL_OPTIONS='-Ddevdock.keep=yes' TEST_CAPTURE="$captured_file" /bin/bash "$script_dir/build.sh" "$resources" "$android_root" simpleDemo "$jdk" assembleRelease __UNI__TEST
  captured=$(<"$captured_file")
  [[ "$captured" == *"JAVA_HOME=$jdk"* ]] || { printf '%s\n' '未使用所选 JDK'; exit 1; }
  [[ "$captured" == *'-Ddevdock.keep=yes'* ]] || { printf '%s\n' '覆盖了已有 JAVA_TOOL_OPTIONS'; exit 1; }
  [[ "$captured" == *'ARG=:simpleDemo:assembleRelease'* ]] || exit 1
  [[ -f "$resources/www/index.html" && -f "$android_root/simpleDemo/src/main/assets/apps/__UNI__TEST/www/index.html" ]] || exit 1
  [[ ! -f "$android_root/simpleDemo/src/main/assets/apps/__UNI__TEST/previous.txt" ]] || exit 1
  if [[ "$major" == 17 ]]; then
    [[ "$captured" == *'--add-opens=java.base/java.io=ALL-UNNAMED'* ]] || { printf '%s\n' 'JDK 17 缺少 java.io 模块兼容参数'; exit 1; }
    [[ "$captured" == *'--add-opens=jdk.compiler/com.sun.tools.javac.processing=ALL-UNNAMED'* ]] || exit 1
  else
    [[ "$captured" != *'--add-opens'* ]] || { printf '%s\n' 'JDK 8 不应接收模块参数'; exit 1; }
  fi
  [[ "$captured" != *'-Dorg.gradle.jvmargs='* ]] || { printf '%s\n' '不应覆盖工程 JVM 参数'; exit 1; }
done
printf '%s\n' 'JDK 8/17 构建环境、兼容参数和资源复制测试通过'
