import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { createUpdaterManifest } from './updater-manifest.mjs';

// 构造最小 tar，不调用打包工具或接触真实签名密钥。
function appArchive(version) {
  const plist = Buffer.from(
    `<plist><dict><key>CFBundleShortVersionString</key><string>${version}</string></dict></plist>`,
  );
  const header = Buffer.alloc(512);
  header.write('DevDock.app/Contents/Info.plist');
  header.write('0000644\0', 100);
  header.write(`${plist.length.toString(8).padStart(11, '0')}\0`, 124);
  header.fill(32, 148, 156);
  header[156] = 48;
  const checksum = header.reduce((sum, byte) => sum + byte, 0);
  header.write(`${checksum.toString(8).padStart(6, '0')}\0 `, 148);
  return gzipSync(
    Buffer.concat([header, plist, Buffer.alloc(512 - (plist.length % 512)), Buffer.alloc(1024)]),
  );
}

function updaterFiles(version = '0.1.0') {
  const installers = [
    { name: `DevDock_${version}_x64-setup.exe`, content: Buffer.from('MZsynthetic NSIS') },
    { name: `DevDock_${version}_universal.dmg`, content: Buffer.from('synthetic DMG') },
  ];
  return [
    ...installers.flatMap((file) => [
      file,
      {
        name: `${file.name}.sha256`,
        content: Buffer.from(
          `${createHash('sha256').update(file.content).digest('hex')}  ${file.name}\n`,
        ),
      },
    ]),
    { name: `${installers[0].name}.sig`, content: Buffer.from('windows-signature\n') },
    { name: 'DevDock.app.tar.gz', content: appArchive(version) },
    { name: 'DevDock.app.tar.gz.sig', content: Buffer.from('mac-signature\n') },
  ];
}
const options = { repository: 'example/devdock', tag: 'v0.1.0', version: '0.1.0' };
const generate = (files = updaterFiles(), changes = {}) =>
  createUpdaterManifest({ ...options, files, ...changes });

test('manifest 使用精确 tag URL、签名内容，两个 Darwin 架构共享 Universal app', () => {
  const manifest = generate();
  assert.equal(manifest.version, '0.1.0');
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    'darwin-aarch64',
    'darwin-x86_64',
    'windows-x86_64',
  ]);
  assert.deepEqual(manifest.platforms['windows-x86_64'], {
    signature: 'windows-signature',
    url: 'https://github.com/example/devdock/releases/download/v0.1.0/DevDock_0.1.0_x64-setup.exe',
  });
  assert.deepEqual(manifest.platforms['darwin-aarch64'], {
    signature: 'mac-signature',
    url: 'https://github.com/example/devdock/releases/download/v0.1.0/DevDock.app.tar.gz',
  });
  assert.deepEqual(manifest.platforms['darwin-aarch64'], manifest.platforms['darwin-x86_64']);
});

test('接受合法预发布版本', () => {
  assert.equal(
    generate(updaterFiles('1.2.3-beta.1'), { version: '1.2.3-beta.1', tag: 'v1.2.3-beta.1' })
      .version,
    '1.2.3-beta.1',
  );
});

test('拒绝非法版本、仓库、tag 与 URL 路径注入', () => {
  for (const version of ['01.2.3', '1.2', '1.2.3+meta', '1.2.3\n', '../1.2.3', '1.2.3-01']) {
    assert.throws(() => generate(undefined, { version }));
  }
  for (const repository of [
    '',
    '../repo',
    'owner/../repo',
    'https://github.com/a/b',
    'a/b?x',
    'a/b#x',
    'a/b\n',
    'a/%2e%2e',
    'a/.',
  ]) {
    assert.throws(() => generate(undefined, { repository }));
  }
  for (const tag of ['v0.1.1', '../v0.1.0', 'v0.1.0?raw=1', 'v0.1.0\n']) {
    assert.throws(() => generate(undefined, { tag }));
  }
});

test('要求每个产物唯一，缺更新包、签名、校验文件时拒绝', () => {
  for (const file of updaterFiles()) {
    assert.throws(
      () => generate(updaterFiles().filter(({ name }) => name !== file.name)),
      file.name,
    );
    assert.throws(() => generate([...updaterFiles(), file]), file.name);
  }
  assert.throws(() =>
    generate([...updaterFiles(), { name: 'other.exe', content: Buffer.from('MZother') }]),
  );
  assert.throws(() =>
    generate([...updaterFiles(), { name: '../DevDock.app.tar.gz', content: Buffer.from('bad') }]),
  );
});

test('拒绝空签名、空安装包、损坏压缩包、版本不符与 SHA 被篡改', () => {
  const mutations = [
    ['DevDock_0.1.0_x64-setup.exe.sig', Buffer.from(' \r\n')],
    ['DevDock.app.tar.gz.sig', Buffer.alloc(0)],
    ['DevDock_0.1.0_x64-setup.exe', Buffer.from('not an exe')],
    ['DevDock_0.1.0_universal.dmg', Buffer.alloc(0)],
    ['DevDock.app.tar.gz', Buffer.from('not gzip')],
    ['DevDock.app.tar.gz', gzipSync(Buffer.from('not tar'))],
    ['DevDock.app.tar.gz', appArchive('0.0.9')],
    [
      'DevDock_0.1.0_x64-setup.exe.sha256',
      Buffer.from(`${'0'.repeat(64)}  DevDock_0.1.0_x64-setup.exe\n`),
    ],
  ];
  for (const [name, content] of mutations) {
    assert.throws(
      () => generate(updaterFiles().map((file) => (file.name === name ? { name, content } : file))),
      name,
    );
  }
  assert.throws(() => generate(updaterFiles('0.0.9')));
});
