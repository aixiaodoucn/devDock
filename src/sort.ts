import type { Project } from './types';

// 拖拽排序只调整位置，返回新数组，避免直接修改原配置。
export function moveProject(projects: Project[], fromId: string, toIndex: number): Project[] {
  const from = projects.findIndex((item) => item.id === fromId);
  if (from < 0) return projects;
  const target = Math.max(0, Math.min(projects.length - 1, toIndex));
  if (from === target) return projects;
  const next = [...projects];
  const [moved] = next.splice(from, 1);
  next.splice(target, 0, moved);
  return next;
}

// 按目标项与鼠标位置计算插入索引；拖到目标下半区时插入其后。
export function dropIndex(projects: Project[], fromId: string, toId: string, after: boolean) {
  const from = projects.findIndex((item) => item.id === fromId);
  const to = projects.findIndex((item) => item.id === toId);
  if (from < 0 || to < 0) return -1;
  let index = to + (after ? 1 : 0);
  if (from < index) index -= 1;
  return index;
}
