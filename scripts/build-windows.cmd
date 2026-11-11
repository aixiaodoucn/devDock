@echo off
setlocal
chcp 65001 >nul

rem 仅在 Windows x64 上验证并构建，不自动安装依赖或修改系统策略。
if not "%OS%"=="Windows_NT" (
  echo 请在 Windows 电脑上运行此脚本。
  exit /b 1
)
cd /d "%~dp0.."
where node >nul 2>nul || goto missing_tools
where pnpm >nul 2>nul || goto missing_tools
where cargo >nul 2>nul || goto missing_tools
where rustc >nul 2>nul || goto missing_tools
where rustup >nul 2>nul || goto missing_tools
node -e "process.exit(parseInt(process.versions.node, 10) >= 22 ? 0 : 1)"
if errorlevel 1 (
  echo 构建 DevDock 需要 Node.js 22 或更高版本。
  exit /b 1
)
rem 直接检查当前 rustc 的目标标准库，不依赖 findstr 对输出换行的匹配。
set "devDockRustLib="
for /f "delims=" %%L in ('rustc --print target-libdir --target x86_64-pc-windows-msvc') do set "devDockRustLib=%%L"
if not defined devDockRustLib goto missing_rust_target
if not exist "%devDockRustLib%\libstd-*.rlib" goto missing_rust_target
if not exist node_modules (
  echo 未安装项目依赖，请先确认并运行 pnpm install --frozen-lockfile。
  exit /b 1
)

call pnpm test
if errorlevel 1 exit /b 1
cargo test --manifest-path src-tauri\Cargo.toml --target x86_64-pc-windows-msvc --lib -- --test-threads=1
if errorlevel 1 exit /b 1
call pnpm exec tauri build --target x86_64-pc-windows-msvc --bundles nsis
if errorlevel 1 exit /b 1
echo.
echo 构建完成，安装包位于 src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\。
exit /b 0

:missing_rust_target
echo 当前 Rust 工具链未找到 x86_64-pc-windows-msvc 标准库，工具链状态如下：
rustup show active-toolchain
rustup target list --installed
rustc --version
echo 请为上述工具链安装目标：rustup target add x86_64-pc-windows-msvc
echo 此检查不判断 MSVC 和 Windows SDK，后续实际编译会报告链接工具错误。
exit /b 1

:missing_tools
echo 请先安装 Node.js 22+、pnpm 11、Rust 1.89+ 与 Visual Studio C++ Build Tools / Windows SDK。
exit /b 1
