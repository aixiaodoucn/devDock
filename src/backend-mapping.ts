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

export function syncBackendModules(
  repositories: BackendRepository[],
  remoteUrls: Record<string, string>,
  development: ScmModule[],
  testing?: ScmModule[],
): { repositories: BackendRepository[]; visibleRepositoryIds: string[]; warnings: string[] } {
  const visibleRepositoryIds: string[] = [];
  const warnings: string[] = [];
  const matchedModules = new Set<string>();
  const updated = repositories.map((repository) => {
    const gitId = repository.scmGitId.trim();
    const url = repositoryUrl(remoteUrls[repository.id] ?? '');
    const matches = development.filter((module) =>
      gitId
        ? module.gitId?.trim() === gitId
        : !!url && repositoryUrl(module.gitRemoteUrl ?? '') === url,
    );
    if (!matches.length) return { ...repository };
    if (matches.length !== 1) {
      warnings.push(`「${repository.name}」无法唯一匹配开发 SCM 模块，请核对仓库编号或远端地址。`);
      return { ...repository };
    }
    const module = matches[0];
    const moduleId = module.gitId?.trim();
    if (!moduleId || !module.branch.trim()) {
      warnings.push(`「${repository.name}」的开发 SCM 仓库编号或分支信息不完整。`);
      return { ...repository };
    }
    visibleRepositoryIds.push(repository.id);
    matchedModules.add(moduleId);
    let testBranch = '';
    // 测试目标只能来自本次明确加载的测试源，不能沿用历史分支。
    if (testing !== undefined) {
      const paired = testing.filter((candidate) => candidate.gitId?.trim() === moduleId);
      if (paired.length === 1 && paired[0].branch.trim()) testBranch = paired[0].branch.trim();
      else {
        testBranch = '';
        warnings.push(
          `「${repository.name}」没有同仓库编号的唯一测试分支，已清空旧测试目标，请配置后再合并。`,
        );
      }
    }
    return {
      ...repository,
      scmGitId: moduleId,
      scmModuleName: module.moduleName,
      developmentBranch: module.branch.trim(),
      testBranch,
    };
  });
  const reported = new Set<string>();
  for (const module of development) {
    const key = module.gitId?.trim() || module.moduleName;
    if ((!module.gitId || !matchedModules.has(module.gitId.trim())) && !reported.has(key)) {
      warnings.push(
        `SCM 模块「${module.moduleName}」未匹配本地仓库，请添加目录或核对 Git ID、远端地址。`,
      );
      reported.add(key);
    }
  }
  return { repositories: updated, visibleRepositoryIds, warnings };
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
      // 未请求测试映射时清空历史目标，测试分支必须来自当前测试源。
      testBranch: paired ? paired[0].branch : '',
    };
  });
}
