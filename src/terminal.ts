import { isWindows } from './platform';

const MACOS_TERMINALS = new Set([
  'Terminal.app',
  'Ghostty.app',
  'iTerm.app',
  'iTerm2.app',
  'WezTerm.app',
  'kitty.app',
  'Alacritty.app',
  'Warp.app',
  'Tabby.app',
  'cmux.app',
]);

// 配置时先提示支持范围；路径存在性和实际启动结果仍由原生端检查。
export function terminalFeedback(path: string, windows = isWindows) {
  const name = path.split(/[\\/]/).at(-1) || '';
  const supported = windows
    ? ['', 'WindowsTerminal', 'PowerShell'].includes(path) ||
      (/^(?:[a-z]:[\\/]|\\\\)/i.test(path) &&
        /^(?:wt|windowsterminal|powershell|pwsh|wezterm|wezterm-gui|alacritty|tabby|warp)\.exe$/i.test(
          name,
        ))
    : ['', 'Terminal'].includes(path) ||
      (path.startsWith('/') &&
        !path.split('/').some((part) => part === '.' || part === '..') &&
        MACOS_TERMINALS.has(name));
  if (!supported || path.includes('\0')) {
    return {
      supported: false,
      message: '此终端尚未适配，执行时会被拒绝。请选择已支持的终端，并保留应用原包名。',
    };
  }
  if (windows && name.toLowerCase() === 'warp.exe') {
    return {
      supported: true,
      message:
        'Warp 使用系统已注册的 warp:// 协议启动；请先安装并完成协议注册，仅有便携版 EXE 不一定可用。',
    };
  }
  if (!windows && name === 'Warp.app') {
    return {
      supported: true,
      message:
        'Warp 通过启动配置创建会话；生成的配置保留在 ~/.warp/launch_configurations/DevDock-*.yaml，可在不再使用时手动清理。',
    };
  }
  if ((!windows && name === 'Tabby.app') || (windows && name.toLowerCase() === 'tabby.exe')) {
    return { supported: true, message: 'Tabby 会弹出执行确认，请点击 Run；取消后不会执行命令。' };
  }
  if (!windows && name === 'cmux.app') {
    return {
      supported: true,
      message:
        '请先启动 cmux，并在 cmux 中允许外部程序访问控制接口；DevDock 不会自动修改该权限。每次请求创建新 workspace。',
    };
  }
  return { supported: true, message: '' };
}
