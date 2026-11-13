import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { tauriArgs } from './tauri.mjs';

const devPath = fileURLToPath(new URL('../src-tauri/tauri.dev.conf.json', import.meta.url));

test('标准开发命令自动使用独立身份配置', () => {
  assert.deepEqual(tauriArgs(['dev']), ['dev', '--config', devPath]);
  assert.deepEqual(tauriArgs(['dev', '--no-watch']), ['dev', '--no-watch', '--config', devPath]);
});

test('开发配置置于应用参数分隔符前，保留用户参数', () => {
  assert.deepEqual(tauriArgs(['dev', '--', '--example']), [
    'dev',
    '--config',
    devPath,
    '--',
    '--example',
  ]);
});

test('正式构建、打包及其他 CLI 命令不注入开发配置', () => {
  for (const args of [['build'], ['build', '--debug'], ['bundle'], ['info'], ['--version']]) {
    assert.deepEqual(tauriArgs(args), args);
  }
});

test('开发与正式应用名称和标识不同，开发配置不覆盖版本和更新配置', () => {
  const production = JSON.parse(
    readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url)),
  );
  const dev = JSON.parse(readFileSync(devPath));
  assert.equal(dev.productName, 'DevDock Dev');
  assert.equal(dev.identifier, 'cn.aixiaodou.devdock.dev');
  assert.equal(production.productName, 'DevDock');
  assert.equal(production.identifier, 'cn.aixiaodou.devdock');
  assert.deepEqual(Object.keys(dev).sort(), ['$schema', 'identifier', 'productName']);
});
