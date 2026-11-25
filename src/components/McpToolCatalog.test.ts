// @vitest-environment happy-dom
import { createApp } from 'vue';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import McpToolCatalog from './McpToolCatalog.vue';

const calls = vi.hoisted(() => ({
  mcpToolCatalog: vi.fn(),
  logMcpStatus: vi.fn(),
  logMcpScanScope: vi.fn(),
  logMcpRawStatus: vi.fn(),
  logScmStatus: vi.fn(),
  logMcpToken: vi.fn(),
  mcpClientStatus: vi.fn(),
}));
vi.mock('../api', () => ({ api: calls }));
const tool = {
  name: 'fixture_lookup',
  title: '查询样本',
  description: '按条件查找样本记录',
  source: '样本库',
  environment: '本地',
  example: '帮我查找昨天的样本',
  inputSchema: {
    type: 'object',
    required: ['query'],
    properties: {
      query: { type: 'string', description: '查询内容' },
      offset: { type: 'integer', default: 0, minimum: 0, maximum: 10 },
      full: { type: 'boolean', default: false },
    },
  },
};
const root = document.createElement('div');
let app: ReturnType<typeof createApp>;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
function mount() {
  app = createApp(McpToolCatalog);
  app.mount(root);
}
async function click(label: string) {
  const button = Array.from(root.querySelectorAll('button')).find(
    (item) => item.textContent?.trim() === label,
  );
  expect(button).toBeDefined();
  button!.click();
  await settle();
}
beforeEach(() => {
  vi.resetAllMocks();
  calls.mcpToolCatalog.mockResolvedValue([tool]);
  calls.logMcpStatus.mockResolvedValue({ enabled: false, endpoint: '' });
});
afterEach(() => {
  app.unmount();
  vi.restoreAllMocks();
  expect(calls.logScmStatus).not.toHaveBeenCalled();
  expect(calls.logMcpToken).not.toHaveBeenCalled();
  expect(calls.mcpClientStatus).not.toHaveBeenCalled();
});

it('本机停止监听也能按来源浏览工具、展开参数并复制后端原文', async () => {
  const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
  calls.mcpToolCatalog.mockResolvedValue([tool, { ...tool, name: 'second' }]);
  mount();
  expect(root.textContent).toContain('正在加载');
  await settle();
  expect(root.textContent).toMatch(/未监听/);
  expect(root.textContent).toMatch(/账号.*客户端/);
  expect(root.querySelectorAll('h3')).toHaveLength(1);
  expect(root.textContent).toContain(tool.environment);
  const details = root.querySelector('details')!;
  expect(details.open).toBe(false);
  details.querySelector('summary')!.click();
  expect(details.open).toBe(true);
  expect(details.textContent).toContain(tool.description);
  expect(details.textContent).toMatch(/query.*string.*必填.*查询内容/s);
  expect(details.textContent).toMatch(/offset.*可选.*默认值\s*0/s);
  expect(details.textContent).toMatch(/full.*默认值\s*false/s);
  await click('复制工具名');
  expect(copy).toHaveBeenLastCalledWith(tool.name);
  await click('复制提问示例');
  expect(copy).toHaveBeenLastCalledWith(tool.example);
});

it('目录加载失败可重试，本机状态读取失败也能查看目录', async () => {
  calls.mcpToolCatalog.mockRejectedValueOnce(new Error('private detail'));
  calls.logMcpStatus.mockRejectedValueOnce(new Error('private detail'));
  mount();
  await settle();
  expect(root.textContent).toContain('监听状态未知');
  expect(root.textContent).not.toContain('private detail');
  await click('重试加载');
  expect(root.textContent).toContain(tool.title);
  expect(calls.mcpToolCatalog).toHaveBeenCalledTimes(2);
  expect(calls.logMcpStatus).toHaveBeenCalledOnce();
  await click('重试状态');
  expect(root.textContent).toContain('未监听');
});

it('目录为空和本机已监听分别展示', async () => {
  calls.mcpToolCatalog.mockResolvedValue([]);
  calls.logMcpStatus.mockResolvedValue({ enabled: true, endpoint: 'local' });
  mount();
  await settle();
  expect(root.textContent).toContain('暂无工具');
  expect(root.textContent).toContain('已监听');
});
