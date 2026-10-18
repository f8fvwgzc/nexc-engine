import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiFetch, apiRequest } from '@/lib/api/client';
import { ContractError } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { agentCardSchema } from '@/schemas/agent';
import {
  agentTemplateSchema,
  secopsOverviewSchema,
  secopsSpanPageSchema,
  type CreateAgentFromTemplateBody,
  type SecopsSpanPage,
  type SpanFilter,
} from '@/schemas/secops';

import { SseFrameParser } from '../audit/sse-frames';
import { parseFeedFrame, type FeedEvent } from './feed';

/** Everything the top of the console shows, refreshed every 15 seconds. */
export const secopsOverviewQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.secops.overview(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/secops/overview`, secopsOverviewSchema, { signal }),
    refetchInterval: 15_000,
  });

/** Spans per page of the explorer. */
export const SPANS_PAGE_SIZE = 50;

/** The explorer's filter as the query string the server reads (empty fields left out). */
export function spanFilterParams(filter: SpanFilter): Record<string, string | undefined> {
  const trimmed = (value: string | undefined) => {
    const v = value?.trim();
    return v ? v : undefined;
  };
  return {
    agent: trimmed(filter.agent),
    kind: filter.kind,
    tool: trimmed(filter.tool),
    host: trimmed(filter.host),
    run: trimmed(filter.run),
    from: trimmed(filter.from),
    to: trimmed(filter.to),
    decision: filter.decision,
  };
}

/** One page of the trace explorer, newest first; `before` continues below the previous page. */
export function secopsSpansPage(
  workspaceId: string,
  filter: SpanFilter,
  before: string | undefined,
  signal?: AbortSignal,
): Promise<SecopsSpanPage> {
  return apiRequest(`/workspaces/${workspaceId}/secops/spans`, secopsSpanPageSchema, {
    signal,
    query: { ...spanFilterParams(filter), limit: SPANS_PAGE_SIZE, before },
  });
}

/** The built-in role templates. */
export const agentTemplatesQuery = () =>
  queryOptions({
    queryKey: qk.secops.templates,
    queryFn: ({ signal }) =>
      apiRequest('/agent-templates', z.array(agentTemplateSchema), { signal }),
    staleTime: 600_000,
  });

/** Makes an agent from a role template; its card comes back with its score. */
export function createAgentFromTemplate(name: string, body: CreateAgentFromTemplateBody) {
  return apiRequest(`/agents/from-template/${encodeURIComponent(name)}`, agentCardSchema, {
    method: 'POST',
    body,
  });
}

/**
 * Reads the workspace's live feed (`GET /workspaces/{wid}/secops/events`, bearer
 * authenticated, so `fetch` rather than `EventSource`) until the signal aborts or the server
 * closes it, handing every span and alert to `onEvent`. Resolves when the stream ends.
 */
export async function readSecopsFeed(
  workspaceId: string,
  onEvent: (event: FeedEvent) => void,
  signal: AbortSignal,
  onOpen?: () => void,
): Promise<void> {
  const path = `/workspaces/${workspaceId}/secops/events`;
  const res = await apiFetch(path, { headers: { Accept: 'text/event-stream' }, signal });
  if (!res.body) throw new ContractError(path, []);
  onOpen?.();
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  const parser = new SseFrameParser();
  for (;;) {
    const { done, value } = await reader.read();
    const chunk = done ? decoder.decode() : decoder.decode(value, { stream: true });
    for (const frame of parser.push(chunk + (done ? '\n\n' : ''))) {
      const event = parseFeedFrame(frame);
      if (event) onEvent(event);
    }
    if (done) return;
  }
}
