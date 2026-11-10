import { describe, expect, it } from 'vitest';
import { stopMenuAction } from './actions';
import type { Action } from './types';

const run: Action = {
  id: 'dingtalk-dev',
  name: '钉钉开发',
  kind: 'menu',
  platform: 'dingtalk',
  command: '',
  envVars: '',
  menuPath: ['运行', '钉钉小程序-开发'],
};
describe('HBuilderX 停止动作', () => {
  it('派生同一菜单路径的停止入口，不修改原始配置', () => {
    const stop = stopMenuAction(run);
    expect(stop).toMatchObject({
      id: run.id,
      name: '停止 · 钉钉开发',
      menuPath: ['运行', '停止钉钉小程序-开发'],
    });
    expect(run.menuPath).toEqual(['运行', '钉钉小程序-开发']);
  });
  it('只替换最后一层菜单名称', () => {
    const stop = stopMenuAction({
      ...run,
      menuPath: ['运行', '运行到小程序模拟器', '微信开发者工具'],
    });
    expect(stop?.menuPath).toEqual(['运行', '运行到小程序模拟器', '停止微信开发者工具']);
  });
  it('终端命令和发行菜单不生成停止按钮', () => {
    expect(
      stopMenuAction({ ...run, kind: 'shell', command: 'npm run dev', menuPath: [] }),
    ).toBeNull();
    expect(
      stopMenuAction({ ...run, menuPath: ['发行', '自定义发行', '钉钉小程序-生产'] }),
    ).toBeNull();
  });
  it('停止命令和不完整路径不重复派生', () => {
    expect(stopMenuAction({ ...run, menuPath: ['运行', '停止钉钉小程序-开发'] })).toBeNull();
    expect(stopMenuAction({ ...run, menuPath: ['运行'] })).toBeNull();
  });
});
