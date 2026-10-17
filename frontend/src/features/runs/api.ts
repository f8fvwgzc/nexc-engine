import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { qk } from '@/lib/query-keys';
import { artifactSchema, isRunActive, runSchema, type Artifact } from '@/schemas/run';
import { tracePageSchema, traceVerificationSchema, type Span } from '@/schemas/trace';

/** Spans fetched per request, and the most pages read for one run (20,000 spans). */
const TRACE_PAGE = 1000;
const TRACE_MAX_PAGES = 20;

async function fetchTrace(runId: string, signal: AbortSignal): Promise<Span[]> {
  const spans: Span[] = [];
  let after: string | undefined;
  for (let page = 0; page < TRACE_MAX_PAGES; page += 1) {
    const res = await apiRequest(`/runs/${runId}/trace`, tracePageSchema, {
      signal,
      query: { limit: TRACE_PAGE, after },
    });
    spans.push(...res.spans);
    if (!res.next) break;
    after = res.next;
  }
  return spans;
}

/** Every span of a run, in id order. Live runs add to it from `trace.span` events. */
export const runTraceQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.trace(runId),
    queryFn: ({ signal }) => fetchTrace(runId, signal),
  });

/** The run's hash chain, recomputed by the server. */
export const traceVerifyQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.verify(runId),
    queryFn: ({ signal }) =>
      apiRequest(`/runs/${runId}/trace/verify`, traceVerificationSchema, { signal }),
  });

/** `spans` with `span` added (or replaced), kept in id order. */
export function mergeSpan(spans: Span[] | undefined, span: Span): Span[] | undefined {
  if (!spans) return spans;
  const rest = spans.filter((s) => s.id !== span.id);
  rest.push(span);
  return rest.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

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
