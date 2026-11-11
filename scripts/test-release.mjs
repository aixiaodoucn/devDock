import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
  const hashTool = process.platform === 'darwin' ? 'shasum -a 256' : '/usr/bin/sha256sum';
  writeFileSync(join(bin, 'sha256sum'), `#!/bin/bash\nexec ${hashTool} "$@"\n`, { mode: 0o700 });
  for (const [platform, filename] of [
    ['windows', `DevDock_${version}_x64-setup.exe`],
    ['macos', `DevDock_${version}_universal.dmg`],
  ]) {
    if (platform === 'macos' && omitMac) continue;
    const folder = join(dir, 'artifacts', platform);
    mkdirSync(folder, { recursive: true });
    const content = `synthetic ${platform} installer`;
    writeFileSync(join(folder, filename), content);
    const hash = createHash('sha256').update(content).digest('hex');
    writeFileSync(join(folder, `${filename}.sha256`), `${hash}  ${filename}\n`);
    if (tamper && platform === 'windows') writeFileSync(join(folder, filename), 'changed');
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
      return { ...result, calls };
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
  scenario({}, ({ status, calls, stderr }) => {
    assert.equal(status, 0, stderr);
    const steps = calls.trim().split('\n');
    assert.deepEqual(
      steps.map((line) => line.split(' ')[1]),
      ['view', 'create', 'upload', 'edit'],
    );
    assert.match(steps[1], /--verify-tag --draft/);
    assert.match(steps[2], /\.exe/);
    assert.match(steps[2], /\.dmg/);
    assert.match(steps[3], /--draft=true/);
    assert.doesNotMatch(calls, /--draft=false/);
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
test('Tag 发布必须经过版本检查和两个平台构建', () => {
  assert.match(workflow, /tags:\s*\n\s*- 'v\*'/);
  assert.match(workflow, /check --tag "\$RELEASE_TAG"/);
  assert.match(workflow, /needs: \[validate-version, windows, macos\]/);
  assert.match(workflow, /uses: \.\/.github\/workflows\/windows-installer.yml/);
  assert.match(workflow, /uses: \.\/.github\/workflows\/macos-installer.yml/);
  assert.match(workflow, /concurrency:/);
  assert.doesNotMatch(workflow, /--draft=false/);
});
