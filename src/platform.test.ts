import { describe, expect, it } from 'vitest';
import { detectHostPlatform, isAbsolutePath, pathKey } from './platform';
import { defaultConfig, mergeConfig, newProject } from './types';

describe('跨平台路径与界面宿主', () => {
  it('识别 Windows / macOS 宿主，不把普通浏览器默认当成 Windows', () => {
    expect(detectHostPlatform('Mozilla Windows NT 10.0 Win64 x64')).toBe('windows');
    expect(detectHostPlatform('Mozilla Macintosh Intel Mac OS X')).toBe('macos');
    expect(detectHostPlatform('Linux x86_64')).toBe('other');
  });
  it('接受盘符、UNC 和 Unix 绝对路径，拒绝相对路径及空字符', () => {
    for (const path of [
      String.raw`C:\项目 空格\demo`,
      'D:/workspace/project',
      String.raw`\\server\share\project`,
      '/workspace/project',
    ]) {
      expect(isAbsolutePath(path)).toBe(true);
    }
    for (const path of ['project', 'C:project', String.raw`\project`, '', 'C:/bad\0path']) {
      expect(isAbsolutePath(path)).toBe(false);
    }
  });
  it('Windows 大小写、分隔符和扩展前缀等价，Unix 大小写不同', () => {
    expect(pathKey(String.raw`C:\Workspace\Demo` + '\\')).toBe(pathKey('c:/workspace/demo'));
    expect(pathKey(String.raw`\\?\C:\Workspace\Demo`)).toBe(pathKey('c:/workspace/demo'));
    expect(pathKey(String.raw`\\?\UNC\Server\Share\Demo`)).toBe(
      pathKey(String.raw`\\server\share\demo`),
    );
    expect(pathKey('C:/')).toBe('c:/');
    expect(pathKey('/Project')).not.toBe(pathKey('/project'));
    expect(pathKey('/')).toBe('/');
  });
  it('备份合并匹配同一 Windows 目录但保留本机项目 ID', () => {
    const current = {
      ...defaultConfig(),
      projects: [{ ...newProject(), id: 'local', path: String.raw`C:\Workspace\Demo` }],
    };
    const incoming = {
      ...defaultConfig(),
      projects: [
        { ...newProject(), id: 'remote', name: 'Windows 项目', path: 'c:/workspace/demo/' },
      ],
    };
    const merged = mergeConfig(current, incoming);
    expect(merged.projects).toHaveLength(1);
    expect(merged.projects[0]).toMatchObject({ id: 'local', name: 'Windows 项目' });
    expect(current.projects[0].path).toBe(String.raw`C:\Workspace\Demo`);
  });
});
