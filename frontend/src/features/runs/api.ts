import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { saveDownload } from '@/lib/api/download';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import {
  artifactSchema,
  isRunActive,
  runSchema,
  workspaceRunSchema,
  type Artifact,
  type Run,
  type WorkspaceRun,
} from '@/schemas/run';
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
    staleTime: FRESH.list,
    queryFn: ({ signal }) => fetchTrace(runId, signal),
  });

/** The run's hash chain, recomputed by the server. */
export const traceVerifyQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.verify(runId),
    staleTime: FRESH.list,
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

/** Whether any of a graph's runs is still queued or running. */
const runsLive = (query: { state: { data?: Run[] } }) =>
  query.state.data?.some((r) => isRunActive(r.status)) ?? false;

/**
 * A graph's runs, newest first; only the latest `limit` of them when it is given (the issue page
 * needs one). A focus refetches only while a run is live, and a live run is polled while it
 * lasts: the run-status stream is graph-scoped and only the canvas holds it.
 */
export const graphRunsQuery = (graphId: string, limit?: number) =>
  queryOptions({
    queryKey: limit ? [...qk.graphs.runs(graphId), limit] : qk.graphs.runs(graphId),
    queryFn: ({ signal }): Promise<Run[]> =>
      apiRequest(`/graphs/${graphId}/runs`, z.array(runSchema), {
        signal,
        query: { limit },
      }),
    staleTime: (query) => (runsLive(query) ? FRESH.live : FRESH.list),
    refetchOnWindowFocus: runsLive,
    refetchInterval: (query) => (runsLive(query) ? FRESH.live : false),
  });

/** How many runs the Runs page lists. */
export const WORKSPACE_RUNS = 100;

/**
 * The latest runs across the workspace's graphs, newest first, each with its graph's name: one
 * request for the Runs page. Polled while one of them is live, as a graph's runs are.
 */
export const workspaceRunsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.runs.workspace(workspaceId),
    queryFn: ({ signal }): Promise<WorkspaceRun[]> =>
      apiRequest(`/workspaces/${workspaceId}/runs`, z.array(workspaceRunSchema), {
        signal,
        query: { limit: WORKSPACE_RUNS },
      }),
    staleTime: (query) => (runsLive(query) ? FRESH.live : FRESH.list),
    refetchOnWindowFocus: runsLive,
    refetchInterval: (query) => (runsLive(query) ? FRESH.live : false),
  });

export const runQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.detail(runId),
    staleTime: FRESH.live,
    queryFn: ({ signal }) => apiRequest(`/runs/${runId}`, runSchema, { signal }),
    // Poll while the run is live (the SSE stream is graph-scoped; this page may not hold it).
    refetchInterval: (query) =>
      query.state.data && isRunActive(query.state.data.status) ? 3_000 : false,
  });

export const artifactsQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.runs.artifacts(runId),
    staleTime: FRESH.list,
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
