import { queryOptions } from '@tanstack/react-query';

import { apiRequest } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import {
  ontologySessionSchema,
  ontologyStatusSchema,
  type OntologySession,
} from '@/schemas/ontology';

/** How the workspace's ontology is kept, with the open schema proposals. */
export const ontologyStatusQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: ['ontology', workspaceId, 'status'] as const,
    staleTime: FRESH.list,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/ontology`, ontologyStatusSchema, { signal }),
  });

/** The soonest a session is renewed again (a server that mints very short ones, a bad clock). */
const RENEW_FLOOR_MS = 10_000;
/** The longest delay a browser timer takes. */
const TIMER_MAX_MS = 2 ** 31 - 1;

/** What the renewal looks at of the session query. */
interface SessionQuery {
  state: { data?: OntologySession; dataUpdatedAt: number };
}

/**
 * When a session minted at `mintedAt` (ms) is renewed: once three quarters of its life are
 * spent, whatever lifetime the server gives it (`NEXC_ONTO_SESSION_TTL_SECS`, 60 s to 12 h).
 */
export function renewalDelay(expiresAt: string | undefined, mintedAt: number): number {
  const left = expiresAt ? Date.parse(expiresAt) - mintedAt : Number.NaN;
  if (!Number.isFinite(left)) return RENEW_FLOOR_MS;
  return Math.min(TIMER_MAX_MS, Math.max(RENEW_FLOOR_MS, Math.floor(left * 0.75)));
}

/**
 * The member's own session for onto-intelli's UI. It is never kept in browser storage (the key
 * is outside the persisted roots). It is renewed from its own expiry while the page stays open,
 * and on return to a tab that was away past that point (timers do not run in a hidden tab).
 */
export const ontologySessionQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: ['ontology', workspaceId, 'session'] as const,
    staleTime: (query: SessionQuery) =>
      renewalDelay(query.state.data?.expires_at, query.state.dataUpdatedAt),
    gcTime: 0,
    refetchInterval: (query: SessionQuery) =>
      query.state.data
        ? renewalDelay(query.state.data.expires_at, query.state.dataUpdatedAt)
        : false,
    refetchOnWindowFocus: true,
    retry: 1,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/ontology/session`, ontologySessionSchema, {
        method: 'POST',
        signal,
      }),
  });
