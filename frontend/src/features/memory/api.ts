import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { memorySchema, memoryTopicSchema, type MemoryQuery } from '@/schemas/memory';

export const memoriesQuery = (query: MemoryQuery) =>
  queryOptions({
    queryKey: qk.memories.search(query),
    queryFn: ({ signal }) =>
      apiRequest('/memories', z.array(memorySchema), {
        query: {
          workspace_id: query.workspace_id,
          q: query.q,
          graph_id: query.graph_id,
          topic_id: query.topic_id,
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

/** The topics of a workspace's memory, largest first. */
export const memoryTopicsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.memories.topics(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest('/memories/topics', z.array(memoryTopicSchema), {
        query: { workspace_id: workspaceId },
        signal,
      }),
    enabled: workspaceId.length > 0,
  });

/** Starts finding the memory topics afresh; it runs in the background. */
export function rebuildMemoryTopics(workspaceId: string) {
  return apiSend('/memories/topics/rebuild', {
    method: 'POST',
    query: { workspace_id: workspaceId },
  });
}

export function deleteMemory(memoryId: string) {
  return apiSend(`/memories/${memoryId}`, { method: 'DELETE' });
}
