import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { idSchema } from '@/schemas/common';

/** Shortest text the server searches for. */
export const SEARCH_MIN = 2;

export const searchHitSchema = z.object({
  kind: z.enum(['issue', 'project', 'graph', 'document', 'team', 'member']),
  id: idSchema,
  title: z.string(),
  subtitle: z.string(),
});
export type SearchHit = z.infer<typeof searchHitSchema>;

/**
 * What in the workspace matches a few typed characters, as far as the caller may see it: issues,
 * projects, graphs, documents, teams and members, in that order. Outside the roots copied to
 * browser storage: what someone looked for stays in memory.
 */
export const workspaceSearchQuery = (workspaceId: string | undefined, q: string) =>
  queryOptions({
    queryKey: ['search', workspaceId, q] as const,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/search`, z.array(searchHitSchema), {
        query: { q },
        signal,
      }),
    enabled: Boolean(workspaceId) && q.length >= SEARCH_MIN,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
