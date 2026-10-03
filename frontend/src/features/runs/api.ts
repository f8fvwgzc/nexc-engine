import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { qk } from '@/lib/query-keys';
import { artifactSchema, isRunActive, runSchema, type Artifact } from '@/schemas/run';

export const graphRunsQuery = (graphId: string) =>
  queryOptions({
    queryKey: qk.graphs.runs(graphId),
    queryFn: ({ signal }) => apiRequest(`/graphs/${graphId}/runs`, z.array(runSchema), { signal }),
  });

export const runQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.detail(runId),
    queryFn: ({ signal }) => apiRequest(`/runs/${runId}`, runSchema, { signal }),
    // Poll while the run is live (the SSE stream is graph-scoped; this page may not hold it).
    refetchInterval: (query) =>
      query.state.data && isRunActive(query.state.data.status) ? 3_000 : false,
  });

export const artifactsQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.artifacts(runId),
    queryFn: ({ signal }) =>
      apiRequest(`/runs/${runId}/artifacts`, z.array(artifactSchema), { signal }),
  });

export function cancelRun(runId: string) {
  return apiRequest(`/runs/${runId}/cancel`, runSchema, { method: 'POST' });
}

export function downloadArtifact(artifact: Artifact) {
  const fallback = artifact.path.split('/').pop() || 'artifact';
  return saveDownload(`/artifacts/${artifact.id}/download`, fallback);
}

export function downloadArtifactsZip(runId: string) {
  return saveDownload(`/runs/${runId}/artifacts.zip`, `run-${runId.slice(0, 8)}-artifacts.zip`);
}
