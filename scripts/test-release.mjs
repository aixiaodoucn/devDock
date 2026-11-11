import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const workflow = readFileSync(join(root, '.github/workflows/release.yml'), 'utf8');
const marker = '        run: |\n';
const block = workflow.slice(workflow.lastIndexOf(marker) + marker.length);
const publishScript = block
  .split('\n')
  .map((line) => (line.startsWith('          ') ? line.slice(10) : line))
  .join('\n');

// 发布任务仅运行在 Linux；本地 macOS 可测试同样的 Bash 流程，所有 gh 调用均为替身。
function fixture({
  state = 'absent',
  failUpload = false,
  tamper = false,
  omitMac = false,
  version = '0.1.0',
  omitSignature = false,
  emptySignature = false,
  omitUpdate = false,
  duplicateUpdate = false,
  nestedMac = false,
} = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'devdock-release-'));
  const bin = join(dir, 'bin');
  mkdirSync(bin);
  const log = join(dir, 'gh-calls.log');
  writeFileSync(
    join(bin, 'gh'),
    `#!/bin/bash
printf '%s\\n' "$*" >> "$RELEASE_TEST_LOG"
case "$2" in
  view)
    if [ "$RELEASE_TEST_STATE" = absent ]; then exit 1; fi
    if [ "$RELEASE_TEST_STATE" = published ]; then printf 'false\\n'; else printf 'true\\n'; fi
    ;;
  upload)
    if [ "$RELEASE_TEST_FAIL_UPLOAD" = true ]; then exit 1; fi
    ;;
esac
`,
    { mode: 0o700 },
  );
  for (const [platform, filename] of [
    ['windows', `DevDock_${version}_x64-setup.exe`],
    ['macos', `DevDock_${version}_universal.dmg`],
  ]) {
    if (platform === 'macos' && omitMac) continue;
    const folder = join(dir, 'artifacts', platform);
    mkdirSync(folder, { recursive: true });
    const content = `${platform === 'windows' ? 'MZ' : ''}synthetic ${platform} installer`;
    writeFileSync(join(folder, filename), content);
    const hash = createHash('sha256').update(content).digest('hex');
    writeFileSync(join(folder, `${filename}.sha256`), `${hash}  ${filename}\n`);
    if (tamper && platform === 'windows') writeFileSync(join(folder, filename), 'changed');
  }
  // 在隔离目录复制真实生成器，确保发布替身验证同一份生产代码。
  mkdirSync(join(dir, 'scripts'));
  for (const name of ['updater-manifest.mjs', 'version.mjs']) {
    if (existsSync(join(root, 'scripts', name)))
      copyFileSync(join(root, 'scripts', name), join(dir, 'scripts', name));
  }
  if (!omitMac && !omitUpdate) {
    const app = join(dir, 'app', 'DevDock.app', 'Contents');
    mkdirSync(app, { recursive: true });
    writeFileSync(
      join(app, 'Info.plist'),
      `<plist><dict><key>CFBundleShortVersionString</key><string>${version}</string></dict></plist>`,
    );
    const archive = join(dir, 'artifacts', 'macos', 'DevDock.app.tar.gz');
    const packed = spawnSync('tar', ['-czf', archive, '-C', join(dir, 'app'), 'DevDock.app']);
    assert.equal(packed.status, 0, packed.stderr?.toString());
    if (duplicateUpdate) copyFileSync(archive, join(dir, 'artifacts', 'macos', 'other.app.tar.gz'));
  }
  if (!omitSignature) {
    for (const path of [
      join(dir, 'artifacts', 'windows', `DevDock_${version}_x64-setup.exe.sig`),
      ...(!omitMac ? [join(dir, 'artifacts', 'macos', 'DevDock.app.tar.gz.sig')] : []),
    ])
      writeFileSync(path, emptySignature ? ' \n' : 'synthetic-signature\n');
  }
  if (nestedMac && !omitMac) {
    const mac = join(dir, 'artifacts', 'macos');
    for (const folder of ['dmg', 'macos']) mkdirSync(join(mac, folder));
    for (const name of [
      `DevDock_${version}_universal.dmg`,
      `DevDock_${version}_universal.dmg.sha256`,
      'DevDock.app.tar.gz',
      'DevDock.app.tar.gz.sig',
    ]) {
      renameSync(join(mac, name), join(mac, name.includes('.dmg') ? 'dmg' : 'macos', name));
    }
  }
  writeFileSync(join(dir, 'publish.sh'), publishScript);
  return {
    run() {
      const result = spawnSync('bash', ['-e', '-o', 'pipefail', join(dir, 'publish.sh')], {
        cwd: dir,
        env: {
          ...process.env,
          PATH: `${bin}:${process.env.PATH}`,
          GH_TOKEN: 'synthetic-token',
          GITHUB_REPOSITORY: 'example/devdock',
          RELEASE_TAG: 'v0.1.0',
          RELEASE_VERSION: '0.1.0',
          RELEASE_PRERELEASE: 'false',
          RELEASE_TEST_LOG: log,
          RELEASE_TEST_STATE: state,
          RELEASE_TEST_FAIL_UPLOAD: String(failUpload),
        },
        encoding: 'utf8',
      });
      let calls = '';
      try {
        calls = readFileSync(log, 'utf8');
      } catch {}
      const manifestPath = join(dir, 'artifacts', 'latest.json');
      const manifest = existsSync(manifestPath)
        ? JSON.parse(readFileSync(manifestPath, 'utf8'))
        : undefined;
      return { ...result, calls, manifest };
    },
    dispose() {
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
function scenario(options, verify) {
  const data = fixture(options);
  try {
    verify(data.run());
  } finally {
    data.dispose();
  }
}
const unixOnly = { skip: process.platform === 'win32' };

test('两个平台校验成功后创建草稿并上传安装包，不自动公开', unixOnly, () => {
  scenario({}, ({ status, calls, stderr, manifest }) => {
    assert.equal(status, 0, stderr);
    const steps = calls.trim().split('\n');
    assert.deepEqual(
      steps.map((line) => line.split(' ')[1]),
      ['view', 'create', 'upload', 'edit'],
    );
    assert.match(steps[1], /--verify-tag --draft/);
    assert.match(steps[2], /\.exe/);
    assert.match(steps[2], /\.dmg/);
    assert.match(steps[2], /\.exe\.sig/);
    assert.match(steps[2], /DevDock\.app\.tar\.gz\.sig/);
    assert.match(steps[2], /artifacts\/latest\.json/);
    assert.equal(manifest.version, '0.1.0');
    assert.equal(manifest.platforms['windows-x86_64'].signature, 'synthetic-signature');
    assert.match(
      manifest.platforms['darwin-aarch64'].url,
      /\/releases\/download\/v0\.1\.0\/DevDock\.app\.tar\.gz$/,
    );
    assert.match(steps[3], /--draft=true/);
    assert.doesNotMatch(calls, /--draft=false/);
  });
});
test('下载的 Mac artifact 保留 bundle 子目录时仍上传完整附件', unixOnly, () => {
  scenario({ nestedMac: true }, ({ status, stderr, calls, manifest }) => {
    assert.equal(status, 0, stderr);
    assert.match(calls, /artifacts\/macos\/dmg\/DevDock_0\.1\.0_universal\.dmg/);
    assert.match(calls, /artifacts\/macos\/macos\/DevDock\.app\.tar\.gz\.sig/);
    assert.equal(manifest.platforms['darwin-x86_64'].signature, 'synthetic-signature');
  });
});

test('上传失败不公开 Release', unixOnly, () => {
  scenario({ failUpload: true }, ({ status, calls }) => {
    assert.notEqual(status, 0);
    assert.match(calls, /release create/);
    assert.match(calls, /release upload/);
    assert.doesNotMatch(calls, /release edit/);
  });
});
test('已公开版本拒绝覆盖，已有草稿可重试', unixOnly, () => {
  scenario({ state: 'published' }, ({ status, calls }) => {
    assert.notEqual(status, 0);
    assert.doesNotMatch(calls, /release (upload|edit|create)/);
  });
  scenario({ state: 'draft' }, ({ status, calls, stderr }) => {
    assert.equal(status, 0, stderr);
    assert.doesNotMatch(calls, /release create/);
    assert.match(calls, /release upload/);
    assert.match(calls, /release edit/);
  });
});
test('安装包被修改或缺少任一平台时，发布前失败', unixOnly, () => {
  for (const options of [{ tamper: true }, { omitMac: true }]) {
    scenario(options, ({ status, calls }) => {
      assert.notEqual(status, 0);
      assert.equal(calls, '');
    });
  }
});
test('产物版本不匹配时不创建 Release', unixOnly, () => {
  for (const version of ['0.0.9', '0.1.01', '10.1.0']) {
    scenario({ version }, ({ status, calls }) => {
      assert.notEqual(status, 0);
      assert.equal(calls, '');
    });
  }
});
test('缺少更新包、签名或签名为空时，任何 GitHub 操作前失败', unixOnly, () => {
  for (const options of [
    { omitSignature: true },
    { emptySignature: true },
    { omitUpdate: true },
    { duplicateUpdate: true },
  ]) {
    scenario(options, ({ status, calls }) => {
      assert.notEqual(status, 0);
      assert.equal(calls, '');
    });
  }
});

test('Tag 发布必须经过版本检查和两个平台构建', () => {
  assert.match(workflow, /tags:\s*\n\s*- 'v\*'/);
  assert.match(workflow, /check --tag "\$RELEASE_TAG"/);
  assert.match(workflow, /needs: \[validate-version, windows, macos\]/);
  assert.match(workflow, /uses: \.\/.github\/workflows\/windows-installer.yml/);
  assert.match(workflow, /uses: \.\/.github\/workflows\/macos-installer.yml/);
  assert.match(workflow, /concurrency:/);
  assert.doesNotMatch(workflow, /--draft=false/);
});
