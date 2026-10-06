import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { memorySchema, type MemoryQuery } from '@/schemas/memory';

export const memoriesQuery = (query: MemoryQuery) =>
  queryOptions({
    queryKey: qk.memories.search(query),
    queryFn: ({ signal }) =>
      apiRequest('/memories', z.array(memorySchema), {
        query: {
          workspace_id: query.workspace_id,
          q: query.q,
          graph_id: query.graph_id,
          limit: query.limit,
        },
        signal,
      }),
    placeholderData: keepPreviousData,
  });

export function deleteMemory(memoryId: string) {
  return apiSend(`/memories/${memoryId}`, { method: 'DELETE' });
}
