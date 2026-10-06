import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import {
  connectionCheckSchema,
  daySummarySchema,
  infrastructureSchema,
  mapItemSchema,
  mapNeighbourhoodSchema,
  type MapKind,
  timelineDaySchema,
  timelineEntrySchema,
  workspaceMapSchema,
} from '@/schemas/insight';

// These keys are outside the roots copied to browser storage: who did what stays on the server.

/** What happened in a workspace on one day (UTC), newest first. */
export const timelineQuery = (workspaceId: string, day: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'timeline', day] as const,
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

export const workspaceMapQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'map'] as const,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/map`, workspaceMapSchema, { signal }),
  });

/** The things of one kind in a workspace, by name, to pick one and follow its ties. */
export const mapItemsQuery = (workspaceId: string, kind: MapKind, q: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'map', kind, 'list', q] as const,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/map/${kind}`, z.array(mapItemSchema), {
        query: { q: q || undefined, limit: 20 },
        signal,
      }),
    placeholderData: keepPreviousData,
  });

/** One thing with everything it is tied to. */
export const mapNeighbourhoodQuery = (workspaceId: string, kind: MapKind, id: string) =>
  queryOptions({
    queryKey: ['insight', workspaceId, 'map', kind, id] as const,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/map/${kind}/${id}`, mapNeighbourhoodSchema, {
        signal,
      }),
  });

export const infrastructureQuery = () =>
  queryOptions({
    queryKey: ['insight', 'infrastructure'] as const,
    queryFn: ({ signal }) => apiRequest('/admin/infrastructure', infrastructureSchema, { signal }),
  });

/** Asks the server whether it can reach a PostgreSQL or Redis URL. The URL is not kept. */
export function checkConnection(url: string) {
  return apiRequest('/admin/infrastructure/check', connectionCheckSchema, {
    method: 'POST',
    body: { url },
  });
}
