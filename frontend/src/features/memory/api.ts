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
          offset: query.offset,
          preview: query.preview,
        },
        signal,
      }),
    placeholderData: keepPreviousData,
  });

/** One memory in full. Read again every time it is opened, so the dialog never shows a stale copy. */
export const memoryQuery = (memoryId: string) =>
  queryOptions({
    queryKey: qk.memories.one(memoryId),
    queryFn: ({ signal }) => apiRequest(`/memories/${memoryId}`, memorySchema, { signal }),
    staleTime: 0,
    gcTime: 0,
  });

export function deleteMemory(memoryId: string) {
  return apiSend(`/memories/${memoryId}`, { method: 'DELETE' });
}
