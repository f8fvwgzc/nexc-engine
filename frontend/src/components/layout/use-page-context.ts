import { useQuery } from '@tanstack/react-query';
import { useLocation, useMatches } from 'react-router-dom';

import { artifactFileQuery } from '@/features/artifacts/api';
import { graphQuery } from '@/features/graphs/api';
import { issueQuery, projectsQuery } from '@/features/issues/api';
import { teamsQuery } from '@/features/workspaces/api';
import type { PageContext } from '@/schemas/assistant';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { isRouteHandle } from './route-handle';

/**
 * Where the member is, said the way the breadcrumbs say it (`Graphs › Research report`,
 * `Issues › Engineering › ENG-7 › ENG-9 Pick the bus`), so the assistant knows what "this
 * graph" or "this issue" refers to.
 */
export function usePageContext(): PageContext {
  const { pathname } = useLocation();
  const matches = useMatches();
  const params = matches[matches.length - 1]?.params ?? {};
  const graphId = params.graphId ?? '';
  const runId = params.runId ?? '';
  const issueId = params.issueId ?? '';
  const fileId = params.fileId ?? '';
  const projectId = params.projectId ?? '';
  const workspaceId = useWorkspaceStore((s) => s.currentId) ?? '';
  // Read the caches the graph, issue, project and file pages fill; never trigger their own fetch.
  const { data: graphName } = useQuery({
    ...graphQuery(graphId),
    enabled: false,
    select: (g) => g.name,
  });
  const { data: issue } = useQuery({ ...issueQuery(issueId), enabled: false });
  const { data: teams } = useQuery({ ...teamsQuery(issue?.workspace_id ?? ''), enabled: false });
  const { data: projectName } = useQuery({
    ...projectsQuery(workspaceId),
    enabled: false,
    select: (projects) => projects.find((p) => p.id === projectId)?.name,
  });
  const { data: fileName } = useQuery({
    ...artifactFileQuery(fileId),
    enabled: false,
    select: (f) => f.name,
  });
  const labels = matches
    .flatMap((m) => (isRouteHandle(m.handle) ? m.handle.crumbs : []))
    .flatMap((crumb) => {
      if ('label' in crumb) return crumb.label;
      if (crumb.dynamic === 'graph') return graphName ?? 'Graph';
      if (crumb.dynamic === 'issue') {
        if (!issue) return 'Issue';
        return [
          teams?.find((t) => t.id === issue.team_id)?.name ?? 'Team',
          ...(issue.parent ? [issue.parent.identifier] : []),
          `${issue.identifier} ${issue.title}`,
        ];
      }
      if (crumb.dynamic === 'project') return projectName ?? 'Project';
      if (crumb.dynamic === 'artifact') return fileName ?? 'File';
      return `Run ${runId.slice(0, 8)}`;
    });
  return { path: pathname, title: labels.join(' › ') };
}
