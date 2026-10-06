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
  /** Suspended by a platform administrator: they cannot sign in until that is lifted. */
  suspended: z.boolean(),
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

/** The policy a workspace puts on its use of LLMs. */
export const guardrailsSchema = z.object({
  /** Tokens per calendar month for the whole workspace; null for no limit. */
  monthly_token_budget: z.number().int().nonnegative().nullable(),
  member_monthly_token_budget: z.number().int().nonnegative().nullable(),
  /** Providers work may run on; empty allows all. */
  allowed_providers: z.array(z.enum(['anthropic', 'openai_compatible', 'claude_code', 'demo'])),
  allow_code_exec: z.boolean(),
  redact_secrets: z.boolean(),
  /** The most memories the workspace keeps; null keeps all. */
  memory_limit: z.number().int().nullable(),
  /** Memories untouched for this many days are forgotten; null never forgets by age. */
  memory_forget_after_days: z.number().int().nullable(),
});
export type Guardrails = z.infer<typeof guardrailsSchema>;
