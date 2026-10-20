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
  'platform_owner_assigned',
  'trace_sealed',
  'policy_changed',
  'policy_mismatch',
  'agent_quarantined',
  'agent_released',
  'classification_changed',
  'approval_granted',
  'approval_denied',
  'artifact_quarantined',
  'artifact_released',
  'alert_raised',
  'alert_destination_changed',
  'agent_created',
  'agent_deleted',
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

/* ---------- Ask the audit (CONTRACT §4) ---------- */

/** How an answer was produced: from the graph alone (0 tokens) or by a model that explained. */
export const answerModeSchema = z.enum(['deterministic', 'llm']);
export type AnswerMode = z.infer<typeof answerModeSchema>;

/** A fact an answer rests on, as a thing in nexc; `withheld` for data outside the asker's clearance. */
export const auditFactSchema = z.object({
  id: z.string(),
  kind: z.string(),
  label: z.string(),
  link: z.string().nullable(),
});
export type AuditFact = z.infer<typeof auditFactSchema>;

export const auditAnswerSchema = z.object({
  answer: z.string(),
  mode: answerModeSchema,
  facts: z.array(auditFactSchema),
  tokens: z.number().int().nonnegative(),
  latency_ms: z.number().nonnegative(),
  /** Claims without a fact cited: a model's words, not the record. */
  unverified: z.boolean(),
  /** The canned question this was recognised as, when it was one. */
  question: z.string().nullable(),
});
export type AuditAnswer = z.infer<typeof auditAnswerSchema>;

/** A question offered as a chip; `{run}` and `{agent}` are slots the asker fills. */
export const cannedQuestionSchema = z.object({
  id: z.string(),
  label: z.string(),
});
export type CannedQuestion = z.infer<typeof cannedQuestionSchema>;

/** The six questions every workspace asks, as the backend offers them (its list wins when it answers). */
export const AUDIT_QUESTIONS: CannedQuestion[] = [
  { id: 'restricted_reads', label: 'Which agents read restricted data this week?' },
  { id: 'egress_denied', label: 'Which agents called hosts outside the allow-list?' },
  { id: 'run_denied', label: 'Why was run {run} denied?' },
  { id: 'agent_day', label: 'What did agent {agent} do today?' },
  { id: 'code_policies', label: 'Which policies allow code execution?' },
  { id: 'approvals', label: 'Who approved what this month?' },
];
