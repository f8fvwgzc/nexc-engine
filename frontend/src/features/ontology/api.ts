import { queryOptions } from '@tanstack/react-query';

import { apiRequest } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import { ontologySessionSchema, ontologyStatusSchema } from '@/schemas/ontology';

/** How the workspace's ontology is kept, with the open schema proposals. */
export const ontologyStatusQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: ['ontology', workspaceId, 'status'] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/ontology`, ontologyStatusSchema, { signal }),
  });

/** A session that lives an hour by default is renewed well before it ends. */
const SESSION_RENEW_MS = 20 * 60_000;

/**
 * The member's own session for onto-intelli's UI. It is never kept in browser storage (the key
 * is outside the persisted roots) and is renewed while the page stays open.
 */
export const ontologySessionQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: ['ontology', workspaceId, 'session'] as const,
    staleTime: SESSION_RENEW_MS,
    gcTime: 0,
    refetchInterval: SESSION_RENEW_MS,
    refetchOnWindowFocus: false,
    retry: 1,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/ontology/session`, ontologySessionSchema, {
        method: 'POST',
        signal,
      }),
  });
