import type { AiTool } from './types';

export function resolveAiTool(tools: AiTool[], projectToolId: string, defaultToolId: string) {
  // 项目已单独选择但工具失效时，不擅自改用另一个全局工具。
  const id = projectToolId || defaultToolId;
  return tools.find((tool) => tool.id === id && tool.enabled);
}
