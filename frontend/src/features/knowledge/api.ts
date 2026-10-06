import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  documentSchema,
  knowledgeSettingsSchema,
  passageSchema,
  topicSchema,
  type KnowledgeSettingsInput,
} from '@/schemas/knowledge';

export interface DocumentFilter {
  q?: string;
  limit: number;
  offset: number;
}

/**
 * One page of a workspace's documents. While any of them is still being read or embedded the
 * page is asked for again every two seconds, so progress shows without a reload.
 */
export const documentsQuery = (workspaceId: string, filter: DocumentFilter) =>
  queryOptions({
    queryKey: qk.knowledge.documents(workspaceId, filter),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/documents`, z.array(documentSchema), {
        query: { q: filter.q, limit: filter.limit, offset: filter.offset },
        signal,
      }),
    placeholderData: keepPreviousData,
    refetchInterval: (query) =>
      query.state.data?.some((d) => d.status !== 'ready' && d.status !== 'failed') ? 2_000 : false,
  });

/** Sends one file. The same file sent twice is the same document. */
export function uploadDocument(workspaceId: string, file: File) {
  return apiRequest(`/workspaces/${workspaceId}/documents`, documentSchema, {
    method: 'POST',
    query: { name: file.name },
    body: file,
  });
}

export function deleteDocument(workspaceId: string, documentId: string) {
  return apiSend(`/workspaces/${workspaceId}/documents/${documentId}`, { method: 'DELETE' });
}

/** The passages that best answer `q`; nothing is asked while `q` is empty. */
export const knowledgeSearchQuery = (workspaceId: string, q: string, topicId?: string) =>
  queryOptions({
    queryKey: qk.knowledge.search(workspaceId, q, topicId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/knowledge/search`, z.array(passageSchema), {
        query: { q, limit: 8, topic_id: topicId },
        signal,
      }),
    enabled: q.length > 0,
    staleTime: 0,
  });

/** The workspace's topics, largest first. */
export const topicsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.knowledge.topics(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/knowledge/topics`, z.array(topicSchema), { signal }),
  });

/** Starts finding the topics afresh; it runs in the background. */
export function rebuildTopics(workspaceId: string) {
  return apiSend(`/workspaces/${workspaceId}/knowledge/topics/rebuild`, { method: 'POST' });
}

export const knowledgeSettingsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.knowledge.settings(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/knowledge/settings`, knowledgeSettingsSchema, {
        signal,
      }),
  });

export function saveKnowledgeSettings(workspaceId: string, body: KnowledgeSettingsInput) {
  return apiRequest(`/workspaces/${workspaceId}/knowledge/settings`, knowledgeSettingsSchema, {
    method: 'PUT',
    body,
  });
}
