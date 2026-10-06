import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { graphSchema, graphSummarySchema, type GraphInput } from '@/schemas/graph';
import { orchestratorStatusSchema } from '@/schemas/orchestrator';
import { graphTemplateSchema, type CreateFromTemplateBody } from '@/schemas/template';

/** Graphs the caller can open; scoped to one workspace when its id is given. */
export const graphsQuery = (workspaceId?: string) =>
  queryOptions({
    queryKey: qk.graphs.list(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(
        workspaceId ? `/graphs?workspace_id=${workspaceId}` : '/graphs',
        z.array(graphSummarySchema),
        { signal },
      ),
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
    staleTime: 30 * 60_000,
  });

export const orchestratorQuery = () =>
  queryOptions({
    queryKey: qk.orchestrator,
    queryFn: ({ signal }) =>
      apiRequest('/orchestrator/status', orchestratorStatusSchema, { signal }),
    refetchInterval: 15_000,
  });

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
