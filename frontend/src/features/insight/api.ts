import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import {
  connectionCheckSchema,
  daySummarySchema,
  infrastructureSchema,
  latencyReportSchema,
  timelineDaySchema,
  timelineEntrySchema,
} from '@/schemas/insight';

// These keys are outside the roots copied to browser storage: who did what stays on the server.

/** What happened in a workspace on one day (UTC), newest first. */
export const timelineQuery = (workspaceId: string, day: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'timeline', day] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/timeline`, z.array(timelineEntrySchema), {
        query: { day },
        signal,
      }),
  });

/** How much happened on each of the last `days` days. */
export const timelineDaysQuery = (workspaceId: string, days: number) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'days', days] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/timeline/days`, z.array(timelineDaySchema), {
        query: { days },
        signal,
      }),
  });

/** The kept summary of a day, or `null` when none was written. Reading it spends no tokens. */
export const daySummaryQuery = (workspaceId: string, day: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'summary', day] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/timeline/summary`, daySummarySchema.nullable(), {
        query: { day },
        signal,
      }),
  });

/** Has the workspace's model write the summary of a day anew. This spends tokens. */
export function writeDaySummary(workspaceId: string, day: string) {
  return apiRequest(`/workspaces/${workspaceId}/timeline/summary`, daySummarySchema, {
    method: 'POST',
    query: { day },
  });
}

export const infrastructureQuery = () =>
  queryOptions({
    queryKey: ['insight', 'infrastructure'] as const,
    staleTime: FRESH.setup,
    queryFn: ({ signal }) => apiRequest('/admin/infrastructure', infrastructureSchema, { signal }),
  });

/** p50, p99 and max per endpoint over the last five minutes of the instance that answers. */
export const latencyQuery = () =>
  queryOptions({
    queryKey: ['insight', 'latency'] as const,
    staleTime: FRESH.live,
    queryFn: ({ signal }) => apiRequest('/admin/latency', latencyReportSchema, { signal }),
    refetchInterval: 30_000,
  });

/** Asks the server whether it can reach a PostgreSQL or Redis URL. The URL is not kept. */
export function checkConnection(url: string) {
  return apiRequest('/admin/infrastructure/check', connectionCheckSchema, {
    method: 'POST',
    body: { url },
  });
}
