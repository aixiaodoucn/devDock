import { createHash } from 'node:crypto';
import { lstatSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { validateVersion } from './version.mjs';

function validateRelease({ repository, tag, version }) {
  validateVersion(version);
  if (tag !== `v${version}`) throw new Error('Tag 与版本不匹配');
  // URL 的每个路径段必须是明确的 GitHub 仓库标识，不接受转义、查询或相对路径。
  if (
    typeof repository !== 'string' ||
    !/^[A-Za-z0-9][A-Za-z0-9-]*\/[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(repository) ||
    /[\r\n]/.test(repository)
  ) {
    throw new Error('repository 必须是合法的 owner/repo');
  }
}

function validateMacArchive(content, version) {
  let tar;
  try {
    tar = gunzipSync(content, { maxOutputLength: 1024 * 1024 * 1024 });
  } catch {
    throw new Error('macOS 更新包必须是有效的 gzip 压缩包');
  }
  let plistCount = 0;
  // 只读 tar，不解压到文件系统；验证头校验和、边界及 app 内的真实版本。
  for (let offset = 0; offset + 512 <= tar.length;) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const text = (start, end) => header.subarray(start, end).toString('utf8').replace(/\0.*$/s, '');
    const sizeText = text(124, 136).trim();
    const checksumText = text(148, 156).trim();
    const size = Number.parseInt(sizeText, 8);
    const checksum = header.reduce(
      (sum, byte, index) => sum + (index >= 148 && index < 156 ? 32 : byte),
      0,
    );
    if (
      !/^[0-7]+$/.test(sizeText) ||
      !/^[0-7]+$/.test(checksumText) ||
      checksum !== Number.parseInt(checksumText, 8) ||
      offset + 512 + size > tar.length
    ) {
      throw new Error('macOS 更新包 tar 结构无效');
    }
    const name = `${text(345, 500) ? `${text(345, 500)}/` : ''}${text(0, 100)}`.replace(
      /^\.\//,
      '',
    );
    if (name === 'DevDock.app/Contents/Info.plist' && [0, 48].includes(header[156])) {
      plistCount += 1;
      const plist = tar.subarray(offset + 512, offset + 512 + size).toString('utf8');
      const versions = [
        ...plist.matchAll(/<key>CFBundleShortVersionString<\/key>\s*<string>([^<]+)<\/string>/g),
      ];
      if (versions.length !== 1 || versions[0][1] !== version)
        throw new Error('macOS app 版本与发布版本不匹配');
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  if (plistCount !== 1)
    throw new Error('macOS 更新包必须包含唯一的 DevDock.app/Contents/Info.plist');
}

// 纯函数：验证所有产物后返回 manifest；调用方可在任何网络写入前完成验证。
export function createUpdaterManifest({ repository, tag, version, files }) {
  validateRelease({ repository, tag, version });
  const exe = `DevDock_${version}_x64-setup.exe`;
  const dmg = `DevDock_${version}_universal.dmg`;
  const app = 'DevDock.app.tar.gz';
  const required = [exe, `${exe}.sha256`, `${exe}.sig`, dmg, `${dmg}.sha256`, app, `${app}.sig`];
  const assets = new Map();
  for (const { name, content } of files) {
    if (
      !required.includes(name) ||
      name !== basename(name) ||
      assets.has(name) ||
      !Buffer.isBuffer(content) ||
      content.length === 0
    ) {
      throw new Error(`产物不合法、为空、版本不符或重复：${name}`);
    }
    assets.set(name, content);
  }
  for (const name of required) {
    if (!assets.has(name)) throw new Error(`缺少产物：${name}`);
  }
  for (const name of [exe, dmg]) {
    const hash = createHash('sha256').update(assets.get(name)).digest('hex');
    if (assets.get(`${name}.sha256`).toString('utf8').trim() !== `${hash}  ${name}`) {
      throw new Error(`安装包 SHA256 校验失败：${name}`);
    }
  }
  if (assets.get(exe).subarray(0, 2).toString('ascii') !== 'MZ')
    throw new Error('Windows 更新包必须是 EXE');
  validateMacArchive(assets.get(app), version);
  const platform = (name) => {
    const signature = assets.get(`${name}.sig`).toString('utf8').trim();
    if (!signature || signature.includes('\u0000')) throw new Error(`签名为空或无效：${name}.sig`);
    return { signature, url: `https://github.com/${repository}/releases/download/${tag}/${name}` };
  };
  const macos = platform(app);
  return {
    version,
    platforms: {
      'windows-x86_64': platform(exe),
      'darwin-aarch64': macos,
      'darwin-x86_64': { ...macos },
    },
  };
}

function readArtifacts(directory) {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) throw new Error('产物目录不允许符号链接');
    if (stat.isDirectory()) return readArtifacts(path);
    if (!stat.isFile()) throw new Error('产物必须是普通文件');
    return [{ name, content: readFileSync(path) }];
  });
}

function main(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index]?.replace(/^--/, '');
    if (
      !args[index]?.startsWith('--') ||
      !['repository', 'tag', 'version', 'artifacts', 'output'].includes(key) ||
      !args[index + 1] ||
      Object.hasOwn(options, key)
    ) {
      throw new Error(
        '用法：--repository owner/repo --tag vX.Y.Z --version X.Y.Z --artifacts 目录 --output latest.json',
      );
    }
    options[key] = args[index + 1];
  }
  if (Object.keys(options).length !== 5)
    throw new Error('必须显式指定 repository、tag、version、artifacts、output');
  validateRelease(options);
  // artifact 的公共根目录可能保留 dmg/、macos/ 子目录，因此递归收集后按文件名去重。
  const files = ['windows', 'macos'].flatMap((platform) =>
    readArtifacts(join(options.artifacts, platform)),
  );
  const manifest = createUpdaterManifest({ ...options, files });
  writeFileSync(options.output, `${JSON.stringify(manifest, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`生成更新清单失败：${error.message}\n`);
    process.exitCode = 1;
  }
}
