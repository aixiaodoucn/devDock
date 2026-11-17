import { describe, expect, it } from 'vitest';
import { terminalFeedback } from './terminal';

describe('终端配置提示', () => {
  it('未知应用即使路径合法也提示不支持', () => {
    expect(terminalFeedback('/Applications/Unknown.app', false).supported).toBe(false);
    expect(terminalFeedback('/Applications/改名后的终端.app', false).message).toContain('原包名');
    expect(terminalFeedback('Warp.app', false).supported).toBe(false);
    expect(terminalFeedback('/Applications/Warp.app\0', false).supported).toBe(false);
    expect(terminalFeedback('/Applications/Warp.app/', false).supported).toBe(false);
    expect(terminalFeedback('/Applications/../Warp.app', false).supported).toBe(false);
  });
  it('支持用户安装目录，保留三个终端的交互条件说明', () => {
    expect(terminalFeedback('/Users/test/Applications/Warp.app', false).message).toContain(
      '启动配置',
    );
    expect(terminalFeedback('/Applications/Tabby.app', false).message).toContain('Run');
    expect(terminalFeedback('/Applications/cmux.app', false).message).toContain('外部程序');
  });
  it('继承与既有终端不产生额外提示', () => {
    for (const path of ['', 'Terminal', '/Applications/Ghostty.app', '/Applications/iTerm.app']) {
      expect(terminalFeedback(path, false)).toEqual({ supported: true, message: '' });
    }
  });
  it('按当前平台识别支持范围，Windows 仅接受已有启动入口', () => {
    for (const path of ['PowerShell', 'WindowsTerminal', 'C:\\Program Files\\pwsh.exe']) {
      expect(terminalFeedback(path, true).supported).toBe(true);
      expect(terminalFeedback(path, false).supported).toBe(false);
    }
    expect(terminalFeedback('/Applications/Tabby.app', true).supported).toBe(false);
    expect(terminalFeedback('C:\\Tools\\unknown.exe', true).supported).toBe(false);
  });
});
