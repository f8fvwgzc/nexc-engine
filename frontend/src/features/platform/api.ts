import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { idSchema, timestampSchema } from '@/schemas/common';

/** A workspace as the platform sees it: its owner and its size, never its content. */
export const platformWorkspaceSchema = z.object({
  id: idSchema,
  name: z.string(),
  owner_name: z.string().nullable(),
  owner_email: z.string().nullable(),
  member_count: z.number().int(),
  team_count: z.number().int(),
  issue_count: z.number().int(),
  graph_count: z.number().int(),
  created_at: timestampSchema,
});

export const platformUserSchema = z.object({
  id: idSchema,
  email: z.string(),
  name: z.string(),
  role: z.enum(['admin', 'user']),
  workspace_count: z.number().int(),
  owned_count: z.number().int(),
  locked: z.boolean(),
  created_at: timestampSchema,
});
export type PlatformUser = z.infer<typeof platformUserSchema>;

export interface PlatformPage {
  q?: string;
  limit: number;
  offset: number;
}

// Outside the roots copied to browser storage: other people's accounts stay on the server.
export const platformWorkspacesQuery = (page: PlatformPage) =>
  queryOptions({
    queryKey: ['platform', 'workspaces', page] as const,
    queryFn: ({ signal }) =>
      apiRequest('/admin/workspaces', z.array(platformWorkspaceSchema), {
        query: { ...page },
        signal,
      }),
    placeholderData: keepPreviousData,
  });

export const platformUsersQuery = (page: PlatformPage) =>
  queryOptions({
    queryKey: ['platform', 'users', page] as const,
    queryFn: ({ signal }) =>
      apiRequest('/admin/users', z.array(platformUserSchema), { query: { ...page }, signal }),
    placeholderData: keepPreviousData,
  });

/** Makes an account a platform administrator, or an ordinary user again. */
export function setPlatformRole(userId: string, role: PlatformUser['role']) {
  return apiRequest(`/admin/users/${userId}`, platformUserSchema, {
    method: 'PATCH',
    body: { role },
  });
}
