import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const auditActionSchema = z.enum([
  'workspace_renamed',
  'member_added',
  'member_invited',
  'member_role_changed',
  'member_removed',
  'invite_withdrawn',
  'team_created',
  'team_updated',
  'team_deleted',
  'team_member_set',
  'team_member_removed',
  'credential_set',
  'credential_removed',
  'guardrails_changed',
  'label_deleted',
  'knowledge_changed',
  'workspace_transferred',
]);
export type AuditAction = z.infer<typeof auditActionSchema>;

/** One entry of a workspace's audit log. Names are copies, so they outlive what they name. */
export const auditEntrySchema = z.object({
  id: idSchema,
  action: auditActionSchema,
  actor_id: idSchema.nullable(),
  actor_name: z.string(),
  subject: z.string(),
  detail: z.string(),
  created_at: timestampSchema,
});
export type AuditEntry = z.infer<typeof auditEntrySchema>;
