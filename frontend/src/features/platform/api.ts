import { keepPreviousData, queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { idSchema, timestampSchema } from '@/schemas/common';
import { workspaceRoleSchema } from '@/schemas/workspace';

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
export type PlatformWorkspace = z.infer<typeof platformWorkspaceSchema>;

/** A member as the platform sees them: their role in the workspace and whether they can use it. */
export const platformMemberSchema = z.object({
  user_id: idSchema,
  name: z.string(),
  email: z.string(),
  role: workspaceRoleSchema,
  suspended: z.boolean(),
  platform_admin: z.boolean(),
  joined_at: timestampSchema,
});
export type PlatformMember = z.infer<typeof platformMemberSchema>;

export const platformWorkspaceDetailSchema = platformWorkspaceSchema.extend({
  members: z.array(platformMemberSchema),
  footprint: z.object({
    project_count: z.number().int(),
    document_count: z.number().int(),
    document_bytes: z.number().int(),
    memory_count: z.number().int(),
    run_count: z.number().int(),
  }),
});
export type PlatformWorkspaceDetail = z.infer<typeof platformWorkspaceDetailSchema>;

export const platformUserSchema = z.object({
  id: idSchema,
  email: z.string(),
  name: z.string(),
  role: z.enum(['admin', 'user']),
  workspace_count: z.number().int(),
  owned_count: z.number().int(),
  locked: z.boolean(),
  suspended: z.boolean(),
  suspended_reason: z.string(),
  created_at: timestampSchema,
});
export type PlatformUser = z.infer<typeof platformUserSchema>;

export const platformActionSchema = z.enum([
  'role_changed',
  'account_suspended',
  'account_reactivated',
  'owner_assigned',
  'workspace_deleted',
  'account_erased',
]);
export type PlatformAction = z.infer<typeof platformActionSchema>;

/** One thing a platform administrator did. Names are copies: they outlive what they name. */
export const platformEventSchema = z.object({
  id: idSchema,
  action: platformActionSchema,
  actor_id: idSchema.nullable(),
  actor_name: z.string(),
  subject: z.string(),
  detail: z.string(),
  created_at: timestampSchema,
});
export type PlatformEvent = z.infer<typeof platformEventSchema>;

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

export const platformWorkspaceQuery = (id: string) =>
  queryOptions({
    queryKey: ['platform', 'workspaces', 'one', id] as const,
    queryFn: ({ signal }) =>
      apiRequest(`/admin/workspaces/${id}`, platformWorkspaceDetailSchema, { signal }),
  });

export const platformEventsQuery = (page: PlatformPage) =>
  queryOptions({
    queryKey: ['platform', 'events', page] as const,
    queryFn: ({ signal }) =>
      apiRequest('/admin/events', z.array(platformEventSchema), { query: { ...page }, signal }),
    placeholderData: keepPreviousData,
  });

/** A platform role, a suspension with its reason, or lifting one. */
export type PlatformUserChange =
  { role: PlatformUser['role'] } | { suspended: true; reason: string } | { suspended: false };

/** Changes an account's platform role or suspends it; either ends its sessions at once. */
export function updatePlatformUser(userId: string, change: PlatformUserChange) {
  return apiRequest(`/admin/users/${userId}`, platformUserSchema, {
    method: 'PATCH',
    body: change,
  });
}

/**
 * Deletes an account on its holder's request, as they could from their own profile; `email` must
 * be the account's address.
 */
export function eraseAccount(userId: string, email: string) {
  return apiSend(`/admin/users/${userId}/erase`, { method: 'POST', body: { confirm: email } });
}

/** Makes a registered account an owner of a workspace. */
export function assignOwner(workspaceId: string, email: string) {
  return apiRequest(`/admin/workspaces/${workspaceId}/owners`, platformWorkspaceDetailSchema, {
    method: 'POST',
    body: { email },
  });
}

/** Deletes a workspace with everything in it; `name` must be its exact name. */
export function deletePlatformWorkspace(workspaceId: string, name: string) {
  return apiSend(`/admin/workspaces/${workspaceId}/delete`, {
    method: 'POST',
    body: { confirm: name },
  });
}
