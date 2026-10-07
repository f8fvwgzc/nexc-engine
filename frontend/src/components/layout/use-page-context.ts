import { useQuery } from '@tanstack/react-query';
import { useLocation, useMatches } from 'react-router-dom';

import { graphQuery } from '@/features/graphs/api';
import type { PageContext } from '@/schemas/assistant';

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
  // Reads the cache the graph page fills; never triggers its own fetch.
  const { data: graphName } = useQuery({
    ...graphQuery(graphId),
    enabled: false,
    select: (g) => g.name,
  });
  const labels = matches
    .flatMap((m) => (isRouteHandle(m.handle) ? m.handle.crumbs : []))
    .map((crumb) => {
      if ('label' in crumb) return crumb.label;
      return crumb.dynamic === 'graph' ? (graphName ?? 'Graph') : `Run ${runId.slice(0, 8)}`;
    });
  return { path: pathname, title: labels.join(' › ') };
}
