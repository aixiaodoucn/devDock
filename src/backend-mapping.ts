import type { BackendRepository, ScmModule } from './types';

export function scmProjectId(value: string): string {
  const input = value.trim();
  if (/^\d+$/.test(input)) return input;
  try {
    const url = new URL(input);
    if (!['https:', 'http:'].includes(url.protocol)) throw new Error();
    const id =
      url.searchParams.get('projectId') ??
      url.searchParams.get('id') ??
      url.pathname.match(/\/(?:projects?|project-detail)\/(\d+)(?:\/|$)/i)?.[1];
    if (id && /^\d+$/.test(id)) return id;
  } catch {
    /* 非链接输入统一返回校验提示。 */
  }
  throw new Error('请输入纯数字 SCM 项目 ID 或含项目 ID 的完整链接。');
}

function repositoryUrl(value: string): string {
  const input = value.trim();
  if (!input) return '';
  const ssh = input.match(/^(?:[^@/]+@)?([^:/]+):(.+)$/);
  try {
    const url = new URL(input.includes('://') ? input : ssh ? `ssh://${ssh[1]}/${ssh[2]}` : input);
    const port = url.protocol === 'ssh:' && url.port === '22' ? '' : url.port;
    return `${url.hostname.toLowerCase()}${port ? `:${port}` : ''}/${url.pathname.replace(/^\/+|\/+$/g, '').replace(/\.git$/i, '')}`;
  } catch {
    return '';
  }
}

export function mapBackendModules(
  repositories: BackendRepository[],
  remoteUrls: Record<string, string>,
  development: ScmModule[],
  testing?: ScmModule[],
): BackendRepository[] {
  return repositories.map((repository) => {
    const url = repositoryUrl(remoteUrls[repository.id] ?? '');
    const matches = development.filter((module) =>
      repository.scmGitId
        ? module.gitId === repository.scmGitId
        : !!url && repositoryUrl(module.gitRemoteUrl ?? '') === url,
    );
    if (matches.length !== 1 || !matches[0].gitId || !matches[0].branch.trim())
      throw new Error(`「${repository.name}」无法唯一匹配 SCM 模块，请手动选择 SCM Git ID。`);
    const module = matches[0];
    const paired = testing?.filter((item) => item.gitId === module.gitId);
    if (paired && (paired.length !== 1 || !paired[0].branch.trim()))
      throw new Error(`「${repository.name}」没有同 Git ID 的唯一测试分支。`);
    return {
      ...repository,
      scmGitId: module.gitId!,
      scmModuleName: module.moduleName,
      developmentBranch: module.branch,
      // 未请求测试映射时保留已有目标，不能因只更新开发分支而清空。
      testBranch: paired ? paired[0].branch : repository.testBranch,
    };
  });
}
