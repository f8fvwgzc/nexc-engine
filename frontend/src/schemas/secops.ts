import { z } from 'zod';

import { agentStatusSchema, agentRuntimeSchema, capabilitiesSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { policyDocumentSchema, policyScopeSchema, severitySchema } from './policy';
import { spanKindSchema, spanStatusSchema } from './trace';

/* ---------- the console's overview (CONTRACT §4) ---------- */

export const agentCountsSchema = z.object({
  total: z.number().int().nonnegative(),
  active: z.number().int().nonnegative(),
  paused: z.number().int().nonnegative(),
  over_budget: z.number().int().nonnegative(),
  quarantined: z.number().int().nonnegative(),
});

export const runCountsSchema = z.object({
  total: z.number().int().nonnegative(),
  succeeded: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
  cancelled: z.number().int().nonnegative(),
  active: z.number().int().nonnegative(),
});

export const ruleCountSchema = z.object({ rule: z.string(), count: z.number().int() });
export type RuleCount = z.infer<typeof ruleCountSchema>;

export const riskyAgentSchema = z.object({
  agent_id: idSchema,
  name: z.string(),
  principal_id: z.string(),
  status: agentStatusSchema,
  risk_score: z.number().min(0),
  risk_reasons: z.array(z.string()),
  risk_updated_at: nullableTimestampSchema,
});
export type RiskyAgent = z.infer<typeof riskyAgentSchema>;

export const riskPointSchema = z.object({
  agent_id: idSchema,
  /** `YYYY-MM-DD` (UTC). */
  day: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  score: z.number().min(0),
});
export type RiskPoint = z.infer<typeof riskPointSchema>;

export const secopsOverviewSchema = z.object({
  agents: agentCountsSchema,
  runs_today: runCountsSchema,
  runs_week: runCountsSchema,
  spans_today: z.number().int().nonnegative(),
  denials_today: z.array(ruleCountSchema),
  denials_fortnight: z.array(ruleCountSchema),
  approvals_open: z.number().int().nonnegative(),
  alerts_open: z.array(z.object({ severity: severitySchema, count: z.number().int() })),
  artifacts_quarantined: z.number().int().nonnegative(),
  top_risk: z.array(riskyAgentSchema),
  policy_versions: z.array(
    z.object({
      scope: policyScopeSchema,
      scope_id: idSchema.nullable(),
      version: z.number().int(),
      created_at: timestampSchema,
    }),
  ),
  last_seal: z
    .object({
      run_id: idSchema,
      graph_id: idSchema,
      trace_hash: z.string().regex(/^[0-9a-f]{64}$/),
      sealed_at: nullableTimestampSchema,
    })
    .nullable(),
  risk_history: z.array(riskPointSchema),
  at: timestampSchema,
});
export type SecopsOverview = z.infer<typeof secopsOverviewSchema>;

/* ---------- the trace explorer ---------- */

export const secopsSpanSchema = z.object({
  id: idSchema,
  run_id: idSchema,
  graph_id: idSchema,
  node_id: idSchema,
  agent_id: idSchema.nullable(),
  principal: z.string(),
  kind: spanKindSchema,
  name: z.string(),
  started_at: timestampSchema,
  ended_at: nullableTimestampSchema,
  status: spanStatusSchema,
  tool: z.string().nullable(),
  host: z.string().nullable(),
  decision: z.string().nullable(),
  rule: z.string().nullable(),
  model: z.string().nullable(),
  tokens_in: z.number().int().nullable(),
  tokens_out: z.number().int().nullable(),
});
export type SecopsSpan = z.infer<typeof secopsSpanSchema>;

export const secopsSpanPageSchema = z.object({
  spans: z.array(secopsSpanSchema),
  next: idSchema.nullable(),
});
export type SecopsSpanPage = z.infer<typeof secopsSpanPageSchema>;

export const DECISIONS = ['allow', 'deny', 'require_approval', 'alert'] as const;
export type Decision = (typeof DECISIONS)[number];

/** What the explorer narrows by; every field optional. */
export interface SpanFilter {
  agent?: string;
  kind?: z.infer<typeof spanKindSchema>;
  tool?: string;
  host?: string;
  run?: string;
  from?: string;
  to?: string;
  decision?: Decision;
}

/* ---------- role templates (AGENTSECOPS §4.6) ---------- */

export const competencyQuestionSchema = z.object({
  question: z.string(),
  expected: z.array(z.string()),
});

export const agentTemplateSchema = z.object({
  id: z.string().min(1),
  name: z.string(),
  role: z.string(),
  title: z.string(),
  description: z.string(),
  purpose: z.string(),
  runtime: agentRuntimeSchema,
  budget_tokens: z.number().int().nonnegative(),
  capabilities: capabilitiesSchema,
  policy: policyDocumentSchema,
  knowledge: z.array(z.string()),
  questions: z.array(competencyQuestionSchema),
});
export type AgentTemplate = z.infer<typeof agentTemplateSchema>;

export interface CreateAgentFromTemplateBody {
  workspace_id?: string;
  name?: string;
  model?: string;
  team_id?: string;
  evaluate?: boolean;
}
