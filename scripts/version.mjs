import { appendFileSync, existsSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PROJECT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const VERSION_FILES = [
  'package.json',
  'src-tauri/tauri.conf.json',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
];
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?$/;

export function validateVersion(version) {
  if (typeof version !== 'string' || !SEMVER.test(version) || /[\r\n]/.test(version)) {
    throw new Error('版本必须是合法 SemVer（例如 0.2.0 或 0.2.0-beta.1），不支持 build metadata');
  }
  return version;
}

function jsonVersion(text, label) {
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`${label} JSON 格式错误：${error.message}`);
  }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new Error(`${label} 必须是 JSON 对象`);
  }
  // 只定位根对象键，跳过字符串中的花括号及嵌套依赖版本，保留原始排版。
  const tokens = [...text.matchAll(/"(?:[^"\\]|\\.)*"|[{}\[\]:,]|[^\s{}\[\]:,]+/g)];
  const fields = [];
  let depth = 0;
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index][0];
    if (token === '{' || token === '[') depth += 1;
    if (token === '}' || token === ']') depth -= 1;
    if (
      depth === 1 &&
      token.startsWith('"') &&
      tokens[index + 1]?.[0] === ':' &&
      JSON.parse(token) === 'version'
    ) {
      const value = tokens[index + 2];
      if (!value?.[0].startsWith('"')) throw new Error(`${label} version 必须是字符串`);
      fields.push({ start: value.index, end: value.index + value[0].length });
    }
  }
  if (fields.length !== 1) throw new Error(`${label} 必须有唯一的根 version 字段`);
  return { ...fields[0], version: validateVersion(parsed.version), quote: '"' };
}

function tomlSections(text) {
  const headers = [...text.matchAll(/^[ \t]*(\[\[?[^\r\n]+?\]\]?)[ \t]*(?:#[^\r\n]*)?\r?$/gm)];
  return headers.map((header, index) => ({
    name: header[1],
    start: header.index + header[0].length,
    end: headers[index + 1]?.index ?? text.length,
  }));
}

function tomlField(text, section, key, label) {
  const body = text.slice(section.start, section.end);
  const assignments = [...body.matchAll(new RegExp(`^[ \\t]*${key}[ \\t]*=([^\\r\\n]*)`, 'gm'))];
  if (assignments.length !== 1) throw new Error(`${label} ${key} 必须唯一且存在`);
  const assignment = assignments[0];
  const value = assignment[1].match(/^[ \t]*(["'])([^"'\r\n]*)\1[ \t]*(?:#.*)?$/);
  if (!value) throw new Error(`${label} ${key} 必须是单行字符串`);
  const start = section.start + assignment.index + assignment[0].indexOf('=') + 1;
  const quoteOffset = assignment[1].indexOf(value[1]);
  return {
    start: start + quoteOffset,
    end: start + quoteOffset + value[2].length + 2,
    version: value[2],
    quote: value[1],
  };
}

function cargoVersion(text, label, lock) {
  const sections = tomlSections(text);
  let targets;
  if (lock) {
    targets = sections.filter((section) => {
      if (section.name !== '[[package]]') return false;
      return tomlField(text, section, 'name', label).version === 'dev-dock';
    });
  } else {
    targets = sections.filter((section) => section.name === '[package]');
  }
  if (targets.length !== 1) throw new Error(`${label} 必须有唯一的 dev-dock 根包`);
  const target = targets[0];
  if (tomlField(text, target, 'name', label).version !== 'dev-dock') {
    throw new Error(`${label} [package] name 必须是 dev-dock`);
  }
  if (lock && /^[ \t]*source[ \t]*=/m.test(text.slice(target.start, target.end))) {
    throw new Error(`${label} dev-dock 包含 source，无法确定根包`);
  }
  const field = tomlField(text, target, 'version', label);
  validateVersion(field.version);
  return field;
}

// 纯函数：完整验证四个文件后生成仅替换版本值的文本，不执行文件系统操作。
export function inspectVersions(texts) {
  return VERSION_FILES.map((file, index) => {
    const text = texts[file];
    if (typeof text !== 'string') throw new Error(`缺少文件：${file}`);
    const field = index < 2 ? jsonVersion(text, file) : cargoVersion(text, file, index === 3);
    return { file, text, ...field };
  });
}

export function planVersionUpdate(texts, version) {
  validateVersion(version);
  return Object.fromEntries(
    inspectVersions(texts).map(({ file, text, start, end, quote }) => [
      file,
      text.slice(0, start) + quote + version + quote + text.slice(end),
    ]),
  );
}

export function checkVersions(texts, tag) {
  const entries = inspectVersions(texts);
  const version = entries[0].version;
  if (entries.some((entry) => entry.version !== version)) {
    throw new Error(
      `版本不一致：${entries.map((entry) => `${entry.file}=${entry.version}`).join('，')}`,
    );
  }
  if (tag !== undefined && tag !== `v${version}`) {
    throw new Error(`标签不匹配：期望 v${version}`);
  }
  return { version, tag: `v${version}`, prerelease: version.includes('-') };
}

function readVersions(root) {
  return Object.fromEntries(
    VERSION_FILES.map((file) => [file, readFileSync(resolve(root, file), 'utf8')]),
  );
}

export function setVersion(root, version, write = writeFileSync) {
  validateVersion(version);
  const originals = readVersions(root);
  const updated = planVersionUpdate(originals, version);
  const attempted = [];
  try {
    for (const file of VERSION_FILES) {
      if (updated[file] === originals[file]) continue;
      // 写入本身可能在失败前截断文件，因此当前文件也纳入回滚。
      attempted.push(file);
      write(resolve(root, file), updated[file], 'utf8');
    }
  } catch (error) {
    const failures = [];
    for (const file of attempted.reverse()) {
      try {
        write(resolve(root, file), originals[file], 'utf8');
      } catch (rollbackError) {
        failures.push(`${file}: ${rollbackError.message}`);
      }
    }
    throw new Error(
      `版本写入失败：${error.message}${failures.length ? `；回滚失败：${failures.join('，')}` : '；已回滚'}`,
    );
  }
  return version;
}

export function runCli(
  args,
  { root = PROJECT_ROOT, env = process.env, stdout = process.stdout } = {},
) {
  const [command, ...rest] = args;
  if (command === 'set' && rest.length === 1) {
    const version = setVersion(root, rest[0]);
    stdout.write(`已同步四处版本：${version}\n`);
    return;
  }
  if (command === 'check' && (rest.length === 0 || (rest.length === 2 && rest[0] === '--tag'))) {
    const result = checkVersions(readVersions(root), rest.length ? rest[1] : undefined);
    if (env.GITHUB_OUTPUT) {
      const outputPath = resolve(env.GITHUB_OUTPUT);
      if (
        existsSync(outputPath) &&
        VERSION_FILES.some((file) => realpathSync(resolve(root, file)) === realpathSync(outputPath))
      ) {
        throw new Error('GITHUB_OUTPUT 不能指向版本源文件');
      }
      appendFileSync(
        outputPath,
        `version=${result.version}\ntag=${result.tag}\nprerelease=${result.prerelease}\n`,
        'utf8',
      );
    }
    stdout.write(
      `版本检查通过：${result.version}（标签 ${result.tag}，预发布：${result.prerelease ? '是' : '否'}）\n`,
    );
    return;
  }
  throw new Error('用法：node scripts/version.mjs set <版本> | check [--tag v<版本>]');
}

// 先解析符号链接（macOS 临时目录也可能是链接），再编码空格并兼容 Windows 盘符。
if (
  process.argv[1] &&
  existsSync(resolve(process.argv[1])) &&
  import.meta.url === pathToFileURL(realpathSync(resolve(process.argv[1]))).href
) {
  try {
    runCli(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`版本工具错误：${error.message}\n`);
    process.exitCode = 1;
  }
}
