import { THEME_OPTIONS } from './types';

// 主窗口与菜单栏弹框使用同一组主题变量，避免两处选择或配色不一致。
export function applyTheme(theme: string) {
  document.documentElement.dataset.theme = THEME_OPTIONS.some((option) => option.id === theme)
    ? theme
    : 'lavender';
  const color = getComputedStyle(document.documentElement).getPropertyValue('--sidebar').trim();
  if (color) document.querySelector('meta[name="theme-color"]')?.setAttribute('content', color);
}
