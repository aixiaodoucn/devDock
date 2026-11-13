import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const devConfig = fileURLToPath(new URL('../src-tauri/tauri.dev.conf.json', import.meta.url));
const cli = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url));

export function tauriArgs(args) {
  if (args[0] !== 'dev') return [...args];
  // 配置属于 CLI 参数，必须放在传给应用自身的 -- 参数之前。
  const separator = args.indexOf('--');
  const position = separator < 0 ? args.length : separator;
  return [...args.slice(0, position), '--config', devConfig, ...args.slice(position)];
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  const result = spawnSync(process.execPath, [cli, ...tauriArgs(process.argv.slice(2))], {
    stdio: 'inherit',
  });
  if (result.error) process.stderr.write(`无法启动 Tauri：${result.error.message}\n`);
  process.exitCode = result.status ?? 1;
}
