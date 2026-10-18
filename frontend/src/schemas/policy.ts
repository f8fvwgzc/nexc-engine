import { z } from 'zod';

import { dataLabelSchema, capabilitiesSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';

/* ---------- versioned policies (CONTRACT §4) ---------- */

export const POLICY_SCOPES = ['workspace', 'team', 'agent'] as const;
export const policyScopeSchema = z.enum(POLICY_SCOPES);
export type PolicyScope = z.infer<typeof policyScopeSchema>;

export const ruleActionSchema = z.enum(['alert', 'deny']);
export const severitySchema = z.enum(['low', 'medium', 'high', 'critical']);
export type Severity = z.infer<typeof severitySchema>;
export const PII_KINDS = ['email', 'phone', 'card', 'national_id'] as const;
export const piiKindSchema = z.enum(PII_KINDS);
export const injectionActionSchema = z.enum(['flag', 'block']);
/** What the evidence verifier does with an output that is not verified. */
export const evidenceActionSchema = z.enum(['mark', 'reject']);
export type EvidenceAction = z.infer<typeof evidenceActionSchema>;

const listRuleSchema = z.object({
  allow: z.array(z.string()).nullable().default(null),
  deny: z.array(z.string()).default([]),
});

export const timeWindowSchema = z.object({
  days: z.array(z.number().int().min(1).max(7)),
  start: z.string().regex(/^\d{2}:\d{2}$/, { error: 'HH:MM' }),
  end: z.string().regex(/^\d{2}:\d{2}$/, { error: 'HH:MM' }),
});

const outboundSchema = z.object({
  artifacts: z.boolean().nullable().default(null),
  egress_hosts: z.array(z.string()).nullable().default(null),
});

/**
 * One version of a policy: what it sets; `null`, `[]` and `{}` inherit from the level above.
 * Every field has a default so a partial document written by hand parses to the full shape the
 * server stores.
 */
export const policyDocumentSchema = z.object({
  tools: listRuleSchema.prefault({}),
  egress: listRuleSchema.prefault({}),
  models: listRuleSchema.prefault({}),
  labels: z.array(dataLabelSchema).nullable().default(null),
  limits: z
    .object({
      tokens_per_run: z.number().int().positive().nullable().default(null),
      tokens_per_day: z.number().int().positive().nullable().default(null),
      cost_usd_per_day: z.number().positive().nullable().default(null),
      tool_calls_per_minute: z.number().int().positive().nullable().default(null),
      runs_per_hour: z.number().int().positive().nullable().default(null),
      max_depth: z.number().int().min(0).max(8).nullable().default(null),
      max_fan_out: z.number().int().min(0).max(32).nullable().default(null),
    })
    .prefault({}),
  time_windows: z.array(timeWindowSchema).nullable().default(null),
  require_approval: z
    .object({
      tools: z.array(z.string()).default([]),
      labels: z.array(dataLabelSchema).default([]),
      approvers: z.array(idSchema).default([]),
      timeout_s: z.number().int().min(1).max(86400).nullable().default(null),
    })
    .prefault({}),
  alerts: z
    .object({
      risk_score: z.number().min(0).nullable().default(null),
      quarantine_risk_score: z.number().min(0).nullable().default(null),
      denials_per_run: z.number().int().positive().nullable().default(null),
      novelty: z.number().min(0).max(1).nullable().default(null),
    })
    .prefault({}),
  dlp: z
    .object({
      secrets: ruleActionSchema.nullable().default(null),
      pii: z
        .object({
          enabled: z.boolean().nullable().default(null),
          kinds: z.array(piiKindSchema).nullable().default(null),
          keywords: z.array(z.string()).default([]),
          action: ruleActionSchema.nullable().default(null),
        })
        .prefault({}),
      labels: z.partialRecord(dataLabelSchema, outboundSchema).default({}),
    })
    .prefault({}),
  injection: z
    .object({
      action: injectionActionSchema.nullable().default(null),
      model_check: z.boolean().nullable().default(null),
    })
    .prefault({}),
  /** Evidence-bound outputs: whether the verdict is acted on, the coverage a verified output needs, and what falling short means. */
  evidence: z
    .object({
      required: z.boolean().nullable().default(null),
      min_coverage: z.number().min(0).max(1).nullable().default(null),
      action: evidenceActionSchema.nullable().default(null),
    })
    .prefault({}),
});
export type PolicyDocument = z.infer<typeof policyDocumentSchema>;

/** The document with nothing set. */
export const EMPTY_POLICY: PolicyDocument = policyDocumentSchema.parse({});

export const policySchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  scope: policyScopeSchema,
  scope_id: idSchema.nullable(),
  version: z.number().int().positive(),
  document: policyDocumentSchema,
  author_id: idSchema.nullable(),
  author_name: z.string(),
  created_at: timestampSchema,
});
export type Policy = z.infer<typeof policySchema>;

export const policyVersionsSchema = z.object({
  workspace: z.number().int().nullable(),
  team: z.number().int().nullable(),
  agent: z.number().int().nullable(),
});
export type PolicyVersions = z.infer<typeof policyVersionsSchema>;

/** The resolved rules beside the capabilities, as the per-run token carries them. */
export const rulesSchema = z.object({
  tools_denied: z.array(z.string()),
  egress_denied: z.array(z.string()),
  models_denied: z.array(z.string()),
  require_approval: z.object({
    tools: z.array(z.string()),
    labels: z.array(dataLabelSchema),
    approvers: z.array(idSchema),
    timeout_s: z.number().int(),
  }),
  limits: z.object({
    tokens_per_run: z.number().nullable(),
    tokens_per_day: z.number().nullable(),
    cost_usd_per_day: z.number().nullable(),
    tool_calls_per_minute: z.number().nullable(),
    runs_per_hour: z.number().nullable(),
  }),
  time_windows: z.array(z.array(timeWindowSchema)),
  alerts: z.object({
    risk_score: z.number().nullable(),
    quarantine_risk_score: z.number().nullable(),
    denials_per_run: z.number().nullable(),
    novelty: z.number().nullable(),
  }),
  dlp: z.object({
    secrets: ruleActionSchema,
    pii: z.object({
      enabled: z.boolean(),
      kinds: z.array(piiKindSchema),
      keywords: z.array(z.string()),
      action: ruleActionSchema,
    }),
    labels: z.partialRecord(
      dataLabelSchema,
      z.object({ artifacts: z.boolean(), egress_hosts: z.array(z.string()).nullable() }),
    ),
  }),
  injection: z.object({ action: injectionActionSchema, model_check: z.boolean() }),
  evidence: z.object({
    required: z.boolean(),
    min_coverage: z.number().min(0).max(1),
    action: evidenceActionSchema,
  }),
  versions: policyVersionsSchema,
});
export type Rules = z.infer<typeof rulesSchema>;

/** An agent's effective policy and which level set each rule. */
export const resolvedPolicySchema = z.object({
  capabilities: capabilitiesSchema,
  rules: rulesSchema,
  origins: z.record(z.string(), z.string()),
  levels: z.array(
    z.object({
      scope: policyScopeSchema,
      scope_id: idSchema.nullable(),
      version: z.number().int(),
    }),
  ),
});
export type ResolvedPolicy = z.infer<typeof resolvedPolicySchema>;

/* ---------- approvals ---------- */

export const approvalStatusSchema = z.enum(['pending', 'approved', 'denied', 'expired']);
export type ApprovalStatus = z.infer<typeof approvalStatusSchema>;

export const approvalSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  run_id: idSchema,
  node_id: idSchema,
  graph_id: idSchema,
  agent_id: idSchema.nullable(),
  principal: z.string(),
  span_id: idSchema,
  tool: z.string(),
  args_sha256: z.string().nullable(),
  rule: z.string(),
  reason: z.string(),
  approver_ids: z.array(idSchema),
  status: approvalStatusSchema,
  requested_at: timestampSchema,
  expires_at: timestampSchema,
  decided_by: z.object({ id: idSchema.nullable(), name: z.string() }).nullable(),
  decided_at: nullableTimestampSchema,
  decision_reason: z.string(),
  decision_span_id: idSchema.nullable(),
  policy_versions: policyVersionsSchema,
});
export type Approval = z.infer<typeof approvalSchema>;

/* ---------- alerts ---------- */

export const alertKindSchema = z.enum(['dlp', 'risk', 'injection', 'policy']);
export type AlertKind = z.infer<typeof alertKindSchema>;

export const alertSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  agent_id: idSchema.nullable(),
  principal: z.string(),
  run_id: idSchema.nullable(),
  node_id: idSchema.nullable(),
  graph_id: idSchema.nullable(),
  span_id: idSchema.nullable(),
  kind: alertKindSchema,
  severity: severitySchema,
  rule: z.string(),
  title: z.string(),
  detail: z.string(),
  created_at: timestampSchema,
  acknowledged_at: nullableTimestampSchema,
  acknowledged_by: idSchema.nullable(),
  /** Destinations the alert was delivered to (alert routing); 0 until the worker sent it. */
  deliveries: z.number().int().nonnegative(),
});
export type Alert = z.infer<typeof alertSchema>;

/** The colour of a severity, as a CSS custom property of the status palette. */
export const SEVERITY_META: Record<Severity, { label: string; color: string }> = {
  low: { label: 'Low', color: 'var(--status-idle)' },
  medium: { label: 'Medium', color: 'var(--status-queued)' },
  high: { label: 'High', color: 'var(--status-failed)' },
  critical: { label: 'Critical', color: 'var(--status-failed)' },
};
