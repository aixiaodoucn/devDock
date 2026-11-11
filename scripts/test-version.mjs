import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import {
  checkVersions,
  inspectVersions,
  planVersionUpdate,
  runCli,
  setVersion,
  validateVersion,
} from './version.mjs';

const SCRIPT = fileURLToPath(new URL('./version.mjs', import.meta.url));
const FILES = [
  'package.json',
  'src-tauri/tauri.conf.json',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
];

function fixture(newline = '\n') {
  const texts = {
    'package.json':
      '{\n  "name": "dev-dock", "version" : "0.1.0",\n  "dependencies": {"version": "0.1.0", "other": "^9.0.0"}\n}\n',
    'src-tauri/tauri.conf.json': '{"version":"0.1.0", "app": {"version":"7.0.0"}}\n',
    'src-tauri/Cargo.toml':
      '# 注释保留\n[package]\nname = "dev-dock"\nversion = \'0.1.0\' # 发布版本\n\n[dependencies]\nother = { version = "0.1.0" }\n',
    'src-tauri/Cargo.lock':
      '# 此文件格式保留\nversion = 4\n\n[[package]]\nname = "dev-dock"\nversion = "0.1.0"\ndependencies = ["other"]\n\n[[package]]\nname = "other"\nversion = "0.1.0"\nsource = "registry+https://example.invalid"\n',
  };
  return Object.fromEntries(
    Object.entries(texts).map(([file, text]) => [file, text.replaceAll('\n', newline)]),
  );
}

function tempProject(t, texts = fixture()) {
  const root = mkdtempSync(join(tmpdir(), 'dev dock version # '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [file, text] of Object.entries(texts)) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    writeFileSync(join(root, file), text);
  }
  return root;
}

function readProject(root) {
  return Object.fromEntries(FILES.map((file) => [file, readFileSync(join(root, file), 'utf8')]));
}

function cli(root, args, extraEnv = {}) {
  return spawnSync(process.execPath, [join(root, 'scripts/version.mjs'), ...args], {
    cwd: tmpdir(),
    encoding: 'utf8',
    env: { ...process.env, GITHUB_OUTPUT: '', ...extraEnv },
  });
}

function copyCli(root) {
  mkdirSync(join(root, 'scripts'), { recursive: true });
  copyFileSync(SCRIPT, join(root, 'scripts/version.mjs'));
}

test('SemVer 接受正式版和预发布，拒绝非法值及输出注入', () => {
  for (const version of [
    '0.0.0',
    '0.2.0',
    '12.34.56',
    '0.2.0-beta.1',
    '1.0.0-0',
    '1.0.0-01a',
    '1.0.0-rc-1',
  ]) {
    assert.equal(validateVersion(version), version);
  }
  for (const version of [
    '',
    'v0.2.0',
    '01.2.0',
    '1.02.0',
    '1.2.00',
    '1.0',
    '1.0.0-01',
    '1.0.0-beta.01',
    '1.0.0-',
    '1.0.0-a..b',
    '1.0.0+build',
    '1.0.0\n',
    '1.0.0\r',
    '1.0.0\nsecret=1',
    '1.0.0;touch bad',
    ' 1.0.0',
    null,
    123,
  ]) {
    assert.throws(() => validateVersion(version), /SemVer/);
  }
});

for (const newline of ['\n', '\r\n']) {
  test(`同步四文件并逐字保留依赖、注释、排版及 ${newline === '\n' ? 'LF' : 'CRLF'}`, (t) => {
    const originals = fixture(newline);
    const root = tempProject(t, originals);
    const entries = inspectVersions(originals);
    const planned = planVersionUpdate(originals, '0.2.0-beta.1');
    assert.deepEqual(originals, fixture(newline));
    setVersion(root, '0.2.0-beta.1');
    assert.deepEqual(readProject(root), planned);
    for (const entry of entries) {
      assert.equal(
        planned[entry.file],
        entry.text.slice(0, entry.start) +
          entry.quote +
          '0.2.0-beta.1' +
          entry.quote +
          entry.text.slice(entry.end),
      );
    }
    assert.deepEqual(checkVersions(planned, 'v0.2.0-beta.1'), {
      version: '0.2.0-beta.1',
      tag: 'v0.2.0-beta.1',
      prerelease: true,
    });
  });
}

test('check 和 tag 匹配/不匹配均不修改源文件', (t) => {
  const texts = fixture();
  const root = tempProject(t, texts);
  const output = { write() {} };
  runCli(['check'], { root, env: {}, stdout: output });
  runCli(['check', '--tag', 'v0.1.0'], { root, env: {}, stdout: output });
  for (const tag of ['v0.2.0', '0.1.0', 'v0.1.0\nINJECT=1', '']) {
    assert.throws(
      () => runCli(['check', '--tag', tag], { root, env: {}, stdout: output }),
      /标签不匹配/,
    );
  }
  assert.deepEqual(readProject(root), texts);
  const inconsistent = {
    ...texts,
    'package.json': texts['package.json'].replace('0.1.0', '0.2.0'),
  };
  assert.throws(() => checkVersions(inconsistent), /版本不一致/);
});

const invalidFixtures = [
  [
    '首个 JSON 格式损坏',
    (texts) => {
      texts['package.json'] = '{"version":"0.1.0",}';
    },
  ],
  [
    '第二个 JSON 格式损坏',
    (texts) => {
      texts['src-tauri/tauri.conf.json'] = '{invalid';
    },
  ],
  [
    '重复 JSON 根版本',
    (texts) => {
      texts['package.json'] = '{"version":"0.1.0","version":"0.1.0"}';
    },
  ],
  [
    '非字符串 JSON 版本',
    (texts) => {
      texts['package.json'] = '{"version":123}';
    },
  ],
  [
    'JSON 缺少根版本',
    (texts) => {
      texts['package.json'] = '{"nested":{"version":"0.1.0"}}';
    },
  ],
  [
    '非法已有版本',
    (texts) => {
      texts['package.json'] = '{"version":"01.1.0"}';
    },
  ],
  [
    '重复 Cargo package',
    (texts) => {
      texts['src-tauri/Cargo.toml'] += '\n[package]\nname="dev-dock"\nversion="0.1.0"\n';
    },
  ],
  [
    '重复 Cargo 版本',
    (texts) => {
      texts['src-tauri/Cargo.toml'] = texts['src-tauri/Cargo.toml'].replace(
        '[dependencies]',
        'version="0.1.0"\n[dependencies]',
      );
    },
  ],
  [
    '锁文件没有根包',
    (texts) => {
      texts['src-tauri/Cargo.lock'] = texts['src-tauri/Cargo.lock'].replace('dev-dock', 'renamed');
    },
  ],
  [
    '锁文件重复同名根包',
    (texts) => {
      texts['src-tauri/Cargo.lock'] += '\n[[package]]\nname="dev-dock"\nversion="0.1.0"\n';
    },
  ],
  [
    '锁文件同名带 source 依赖',
    (texts) => {
      texts['src-tauri/Cargo.lock'] +=
        '\n[[package]]\nname="dev-dock"\nversion="9.0.0"\nsource="registry+https://example.invalid"\n';
    },
  ],
  [
    '唯一 dev-dock 也不能有 source',
    (texts) => {
      texts['src-tauri/Cargo.lock'] = texts['src-tauri/Cargo.lock'].replace(
        'name = "dev-dock"',
        'name = "dev-dock"\nsource="git+https://example.invalid"',
      );
    },
  ],
];

for (const [name, mutate] of invalidFixtures) {
  test(`${name}：写入前拒绝且保留原文`, (t) => {
    const texts = fixture();
    mutate(texts);
    const root = tempProject(t, texts);
    let writes = 0;
    assert.throws(() =>
      setVersion(root, '0.2.0', () => {
        writes += 1;
      }),
    );
    assert.equal(writes, 0);
    assert.deepEqual(readProject(root), texts);
    assert.throws(() => checkVersions(texts));
  });
}

test('缺少文件和非法目标版本在任何写入前拒绝', (t) => {
  const texts = fixture();
  const root = tempProject(t, texts);
  let writes = 0;
  const write = () => {
    writes += 1;
  };
  assert.throws(() => setVersion(root, '0.2.0\nINJECT=1', write));
  rmSync(join(root, FILES[3]));
  assert.throws(() => setVersion(root, '0.2.0', write), /ENOENT/);
  assert.equal(writes, 0);
  for (const file of FILES.slice(0, 3))
    assert.equal(readFileSync(join(root, file), 'utf8'), texts[file]);
});

test('写入中途失败时回滚，包括已经被截断的当前文件', (t) => {
  const texts = fixture('\r\n');
  const root = tempProject(t, texts);
  let calls = 0;
  const write = (file, text, encoding) => {
    calls += 1;
    if (calls === 3) {
      writeFileSync(file, 'partial');
      throw new Error('模拟磁盘写入失败');
    }
    writeFileSync(file, text, encoding);
  };
  assert.throws(() => setVersion(root, '0.2.0', write), /已回滚/);
  assert.deepEqual(readProject(root), texts);
});

test('回滚也失败时报告具体文件', (t) => {
  const root = tempProject(t);
  assert.throws(
    () =>
      setVersion(root, '0.2.0', () => {
        throw new Error('只读磁盘');
      }),
    /回滚失败：package.json/,
  );
});

test('JSON 转义根键、嵌套 version 和字符串内括号均准确定位', () => {
  const texts = fixture();
  texts['package.json'] =
    '{"hint":"{} \\\"version\\\"", "nested":[{"version":"8.0.0"}], "vers\\u0069on":"0.1.0"}';
  const updated = planVersionUpdate(texts, '0.2.0');
  assert.equal(updated['package.json'], texts['package.json'].replace('"0.1.0"', '"0.2.0"'));
});

test('CLI 从含空格及 # 的项目路径执行，写安全 CI 输出，错误退出非零', (t) => {
  const root = tempProject(t);
  copyCli(root);
  const output = join(root, 'github output');
  const env = { GITHUB_OUTPUT: output };
  const set = cli(root, ['set', '0.2.0-beta.1'], env);
  assert.equal(set.status, 0, set.stderr);
  assert.match(set.stdout, /已同步四处版本/);
  assert.throws(() => readFileSync(output), /ENOENT/);
  const snapshot = readProject(root);
  const check = cli(root, ['check', '--tag', 'v0.2.0-beta.1'], env);
  assert.equal(check.status, 0, check.stderr);
  assert.match(check.stdout, /版本检查通过/);
  const expected = 'version=0.2.0-beta.1\ntag=v0.2.0-beta.1\nprerelease=true\n';
  assert.equal(readFileSync(output, 'utf8'), expected);
  for (const args of [
    ['check', '--tag', 'v0.2.0'],
    ['check', '--tag', 'v0.2.0-beta.1\nEVIL=1'],
    ['set', '0.2.0\nEVIL=1'],
    ['check', '--tag'],
    ['set'],
    ['unknown'],
  ]) {
    const failed = cli(root, args, env);
    assert.equal(failed.status, 1, failed.stdout);
    assert.match(failed.stderr, /版本工具错误/);
    assert.equal(readFileSync(output, 'utf8'), expected);
    assert.deepEqual(readProject(root), snapshot);
  }
});

test('正式版本 CI 输出 prerelease=false 且追加保留已有输出', (t) => {
  const root = tempProject(t);
  const output = join(root, 'github output');
  writeFileSync(output, 'existing=ok\n');
  runCli(['check'], { root, env: { GITHUB_OUTPUT: output }, stdout: { write() {} } });
  assert.equal(
    readFileSync(output, 'utf8'),
    'existing=ok\nversion=0.1.0\ntag=v0.1.0\nprerelease=false\n',
  );
});

test('配置版本含换行注入时 CLI 不写 CI 输出', (t) => {
  const texts = fixture();
  texts['package.json'] = JSON.stringify({ version: '0.1.0\nEVIL=1' });
  const root = tempProject(t, texts);
  copyCli(root);
  const output = join(root, 'github output');
  const result = cli(root, ['check'], { GITHUB_OUTPUT: output });
  assert.equal(result.status, 1);
  assert.throws(() => readFileSync(output), /ENOENT/);
  assert.deepEqual(readProject(root), texts);
});

test('GITHUB_OUTPUT 不能指向版本源文件，检查失败不修改文件', (t) => {
  const texts = fixture();
  const root = tempProject(t, texts);
  for (const file of FILES) {
    assert.throws(
      () =>
        runCli(['check'], {
          root,
          env: { GITHUB_OUTPUT: join(root, file) },
          stdout: { write() {} },
        }),
      /不能指向版本源文件/,
    );
  }
  assert.deepEqual(readProject(root), texts);
});

test('ESM 导入不运行 CLI', () => {
  const result = spawnSync(
    process.execPath,
    [
      '--input-type=module',
      '-e',
      'await import(process.argv[1])',
      new URL('./version.mjs', import.meta.url).href,
    ],
    { encoding: 'utf8' },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
  assert.equal(result.stderr, '');
});
