export function detectHostPlatform(userAgent: string): 'windows' | 'macos' | 'other' {
  if (/Windows/i.test(userAgent)) return 'windows';
  if (/Macintosh|Mac OS X/i.test(userAgent)) return 'macos';
  return 'other';
}
export const isWindows =
  detectHostPlatform(typeof navigator === 'undefined' ? '' : navigator.userAgent) === 'windows';

function isWindowsPath(path: string) {
  return /^[A-Za-z]:[\\/]/.test(path) || /^(?:\\\\|\/\/)[^\\/]+[\\/][^\\/]+/.test(path);
}
export function isAbsolutePath(path: string) {
  return !path.includes('\0') && (path.startsWith('/') || isWindowsPath(path));
}
export function pathKey(path: string) {
  if (isWindowsPath(path)) {
    const portable = path
      .replaceAll('\\', '/')
      .replace(/^\/\/\?\/UNC\//i, '//')
      .replace(/^\/\/\?\//, '');
    const prefix = portable.startsWith('//') ? '//' : '';
    const normalized =
      prefix +
      portable
        .split('/')
        .filter((part) => part && part !== '.')
        .join('/')
        .toLowerCase();
    return /^[a-z]:$/.test(normalized) ? `${normalized}/` : normalized;
  }
  return path.replace(/\/+$/, '') || (path.startsWith('/') ? '/' : '');
}
