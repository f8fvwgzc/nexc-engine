import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import { graphSchema, graphSummarySchema, type GraphInput } from '@/schemas/graph';
import { plannerStatusSchema } from '@/schemas/planner';
import { graphTemplateSchema, type CreateFromTemplateBody } from '@/schemas/template';

/** Graphs the caller can open; scoped to one workspace when its id is given. */
export const graphsQuery = (workspaceId?: string) =>
  queryOptions({
    queryKey: qk.graphs.list(workspaceId),
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(
        workspaceId ? `/graphs?workspace_id=${workspaceId}` : '/graphs',
        z.array(graphSummarySchema),
        { signal },
      ),
  });

/**
 * The `limit` most recently updated graphs of a workspace (the sidebar's), read as such rather
 * than cut from every graph. Under the workspace's list key, so whatever refreshes the graph
 * lists refreshes these.
 */
export const recentGraphsQuery = (workspaceId: string | undefined, limit: number) =>
  queryOptions({
    queryKey: [...qk.graphs.list(workspaceId), 'recent', limit] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest('/graphs', z.array(graphSummarySchema), {
        signal,
        query: { workspace_id: workspaceId, limit },
      }),
  });

export const graphQuery = (graphId: string) =>
  queryOptions({
    queryKey: qk.graphs.detail(graphId),
    queryFn: ({ signal }) => apiRequest(`/graphs/${graphId}`, graphSchema, { signal }),
    // Realtime keeps the canvas fresh; avoid refetch-on-focus clobbering in-flight drags.
    staleTime: 60_000,
  });

export const templatesQuery = () =>
  queryOptions({
    queryKey: qk.templates,
    queryFn: ({ signal }) => apiRequest('/templates', z.array(graphTemplateSchema), { signal }),
    staleTime: FRESH.catalogue,
  });

export const plannerQuery = () =>
  queryOptions({
    queryKey: qk.planner,
    staleTime: FRESH.live,
    queryFn: ({ signal }) => apiRequest('/planner/status', plannerStatusSchema, { signal }),
  });

/** How often the dashboard's queue and backend health refresh while it is open. */
export const PLANNER_POLL_MS = 15_000;

export function createGraph(body: GraphInput & { workspace_id?: string }) {
  return apiRequest('/graphs', graphSchema, { method: 'POST', body });
}

export function createGraphFromTemplate(body: CreateFromTemplateBody) {
  return apiRequest('/graphs/from-template', graphSchema, { method: 'POST', body });
}

export function updateGraph(graphId: string, body: Partial<GraphInput>) {
  return apiRequest(`/graphs/${graphId}`, graphSchema, { method: 'PATCH', body });
}

export function deleteGraph(graphId: string) {
  return apiSend(`/graphs/${graphId}`, { method: 'DELETE' });
}
