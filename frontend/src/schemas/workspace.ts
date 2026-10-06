import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const workspaceRoleSchema = z.enum(['owner', 'admin', 'member', 'guest']);
export type WorkspaceRole = z.infer<typeof workspaceRoleSchema>;

/** A workspace (organisation) as seen by one of its members. */
export const workspaceSchema = z.object({
  id: idSchema,
  name: z.string(),
  slug: z.string(),
  /** The caller's role. */
  role: workspaceRoleSchema,
  member_count: z.number().int().nonnegative(),
  created_at: timestampSchema,
});
export type Workspace = z.infer<typeof workspaceSchema>;

export const teamRoleSchema = z.enum(['owner', 'member']);
export type TeamRole = z.infer<typeof teamRoleSchema>;

export const workspaceMemberSchema = z.object({
  user_id: idSchema,
  name: z.string(),
  email: z.string(),
  role: workspaceRoleSchema,
  joined_at: timestampSchema,
});
export type WorkspaceMember = z.infer<typeof workspaceMemberSchema>;

/** An invitation that waits for its recipient to sign up. */
export const workspaceInviteSchema = z.object({
  id: idSchema,
  email: z.string(),
  role: workspaceRoleSchema,
  created_at: timestampSchema,
});
export type WorkspaceInvite = z.infer<typeof workspaceInviteSchema>;

export const inviteResultSchema = z.object({
  member: workspaceMemberSchema.nullable(),
  invite: workspaceInviteSchema.nullable(),
});

export const teamSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  name: z.string(),
  /** Short identifier, e.g. ENG. */
  key: z.string(),
  description: z.string(),
  private: z.boolean(),
  /** The caller's role in the team, if they are a member. */
  role: teamRoleSchema.nullable(),
  member_count: z.number().int().nonnegative(),
  created_at: timestampSchema,
});
export type Team = z.infer<typeof teamSchema>;

export const teamMemberSchema = z.object({
  user_id: idSchema,
  name: z.string(),
  email: z.string(),
  role: teamRoleSchema,
  joined_at: timestampSchema,
});
export type TeamMember = z.infer<typeof teamMemberSchema>;

/** Mirrors `domain::workspace` on the server, which stays the authority. */
export const isWorkspaceAdmin = (role: WorkspaceRole) => role === 'owner' || role === 'admin';
