import { queryOptions } from '@tanstack/react-query';

import { apiRequest } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import { isRunActive } from '@/schemas/run';
import {
  agentCancelSchema,
  sessionPageSchema,
  sessionSchema,
  spanPayloadsSchema,
  type SessionFilter,
  type SessionPage,
} from '@/schemas/session';

/** Sessions per page of the list. */
export const SESSIONS_PAGE_SIZE = 50;

/** The list's filter as the query string the server reads (empty fields left out). */
export function sessionFilterParams(filter: SessionFilter): Record<string, string | undefined> {
  const trimmed = (value: string | undefined) => {
    const v = value?.trim();
    return v ? v : undefined;
  };
  return {
    agent: trimmed(filter.agent),
    status: filter.status,
    from: trimmed(filter.from),
    to: trimmed(filter.to),
  };
}

/** One page of a workspace's sessions, newest first; `before` continues below the previous page. */
export function sessionsPage(
  workspaceId: string,
  filter: SessionFilter,
  before: string | undefined,
  signal?: AbortSignal,
): Promise<SessionPage> {
  return apiRequest(`/workspaces/${workspaceId}/sessions`, sessionPageSchema, {
    signal,
    query: { ...sessionFilterParams(filter), limit: SESSIONS_PAGE_SIZE, before },
  });
}

/** A run as a tree. Polls while the run is live; the page also follows its graph's events. */
export const sessionQuery = (runId: string) =>
  queryOptions({
    queryKey: qk.sessions.detail(runId),
    staleTime: FRESH.live,
    queryFn: ({ signal }) => apiRequest(`/runs/${runId}/sessions`, sessionSchema, { signal }),
    refetchInterval: (query) =>
      query.state.data && isRunActive(query.state.data.status) ? 5_000 : false,
  });

/** Stops one sub-agent of a running run, named by its spawn span; the run goes on. */
export function cancelAgent(runId: string, spawnSpanId: string, reason = '') {
  return apiRequest(`/runs/${runId}/agents/${spawnSpanId}/cancel`, agentCancelSchema, {
    method: 'POST',
    body: { reason },
  });
}

/** What a span said, when the workspace records it (the run's owner and admins). */
export const spanPayloadQuery = (spanId: string) =>
  queryOptions({
    queryKey: qk.sessions.payload(spanId),
    queryFn: ({ signal }) => apiRequest(`/spans/${spanId}/payload`, spanPayloadsSchema, { signal }),
    staleTime: 60_000,
  });
