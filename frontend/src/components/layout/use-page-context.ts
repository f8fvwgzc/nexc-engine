import { useQuery } from '@tanstack/react-query';
import { useLocation, useMatches } from 'react-router-dom';

import { artifactFileQuery } from '@/features/artifacts/api';
import { graphQuery } from '@/features/graphs/api';
import { issueQuery, projectsQuery } from '@/features/issues/api';
import type { PageContext } from '@/schemas/assistant';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { isRouteHandle } from './route-handle';

/**
 * Where the member is, said the way the breadcrumbs say it (`Graphs › Research report`), so
 * the assistant knows what "this graph" or "this issue" refers to.
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
  const { data: issueName } = useQuery({
    ...issueQuery(issueId),
    enabled: false,
    select: (i) => `${i.identifier} ${i.title}`,
  });
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
    .map((crumb) => {
      if ('label' in crumb) return crumb.label;
      if (crumb.dynamic === 'graph') return graphName ?? 'Graph';
      if (crumb.dynamic === 'issue') return issueName ?? 'Issue';
      if (crumb.dynamic === 'project') return projectName ?? 'Project';
      if (crumb.dynamic === 'artifact') return fileName ?? 'File';
      return `Run ${runId.slice(0, 8)}`;
    });
  return { path: pathname, title: labels.join(' › ') };
}
