import type { Action } from './types';

// 停止动作只在运行菜单中派生，保留原配置与 ID，由后端再次核对实际运行项目。
export function stopMenuAction(action: Action): Action | null {
  if (action.kind !== 'menu' || action.menuPath[0] !== '运行' || action.menuPath.length < 2)
    return null;
  const leaf = action.menuPath.at(-1);
  if (!leaf || leaf.startsWith('停止')) return null;
  return {
    ...action,
    name: `停止 · ${action.name}`,
    menuPath: [...action.menuPath.slice(0, -1), `停止${leaf}`],
  };
}
