import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { spanSchema } from './trace';

export const AGENT_STATUSES = ['active', 'paused', 'over_budget', 'quarantined'] as const;
export const agentStatusSchema = z.enum(AGENT_STATUSES);
export type AgentStatus = z.infer<typeof agentStatusSchema>;

export const agentRuntimeSchema = z.enum(['python', 'builtin']);
export type AgentRuntime = z.infer<typeof agentRuntimeSchema>;

/** Classification of documents, topics and memories; the clearance an agent holds. */
export const DATA_LABELS = ['public', 'internal', 'confidential', 'restricted'] as const;
export const dataLabelSchema = z.enum(DATA_LABELS);
export type DataLabel = z.infer<typeof dataLabelSchema>;

export const MEMORY_NAMESPACES = ['user', 'graph', 'node'] as const;

/** What an agent may do (CONTRACT §4). `null` on a list means no restriction. */
export const capabilitiesSchema = z.object({
  tools: z.array(z.string()).nullable(),
  data: z.object({
    topics: z.array(idSchema).nullable(),
    labels: z.array(dataLabelSchema),
    memory_namespaces: z.array(z.enum(MEMORY_NAMESPACES)).nullable(),
  }),
  egress_hosts: z.array(z.string()).nullable(),
  counterparts: z.array(z.string()).nullable(),
  delegation: z.object({
    max_depth: z.number().int().min(0).max(8),
    max_fan_out: z.number().int().min(0).max(32),
  }),
  models: z.array(z.string()).nullable(),
});
export type Capabilities = z.infer<typeof capabilitiesSchema>;

/** The default policy: every tool, topic, host and model; public and internal data. */
export const DEFAULT_CAPABILITIES: Capabilities = {
  tools: null,
  data: { topics: null, labels: ['public', 'internal'], memory_namespaces: null },
  egress_hosts: null,
  counterparts: null,
  delegation: { max_depth: 2, max_fan_out: 4 },
  models: null,
};

export const agentSchema = z.object({
  id: idSchema,
  /** `agent://{workspace}/{name}`: the agent's identity across runs and spans. */
  principal_id: z.string(),
  name: z.string(),
  role: z.string(),
  title: z.string(),
  model: z.string(),
  system_prompt: z.string(),
  reports_to: idSchema.nullable(),
  budget_tokens: z.number().int().nonnegative(),
  spent_tokens: z.number().int().nonnegative(),
  status: agentStatusSchema,
  runtime: agentRuntimeSchema,
  heartbeat_at: nullableTimestampSchema,
  created_at: timestampSchema,
  owner_user_id: idSchema.nullable(),
  team_id: idSchema.nullable(),
  purpose: z.string(),
  capabilities: capabilitiesSchema,
  /** Raised with every change to the capabilities, and on quarantine and release. */
  policy_version: z.number().int().nonnegative(),
  quarantined_at: nullableTimestampSchema,
  /** 0–10: how far its latest run strayed from its baseline (0 until three runs were learned). */
  risk_score: z.number().min(0),
  risk_reasons: z.array(z.string()),
  risk_updated_at: nullableTimestampSchema,
});
export type Agent = z.infer<typeof agentSchema>;

/** The competency score an agent made from a role template got on its three questions. */
export const agentEvaluationSchema = z.object({
  template: z.string(),
  score: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
  model: z.string(),
  questions: z.array(
    z.object({
      question: z.string(),
      expected: z.array(z.string()),
      passed: z.boolean(),
      answer: z.string(),
    }),
  ),
  evaluated_at: timestampSchema,
});
export type AgentEvaluation = z.infer<typeof agentEvaluationSchema>;

/** The agent's ID card: who answers for it, its team, what it has left and did last. */
export const agentCardSchema = z.object({
  agent: agentSchema,
  owner: z.object({ id: idSchema, name: z.string() }).nullable(),
  team: z.object({ id: idSchema, name: z.string(), key: z.string() }).nullable(),
  budget_left: z.number().int().nonnegative().nullable(),
  recent_spans: z.array(spanSchema),
  /** The score of an agent made from a role template; `null` otherwise. */
  evaluation: agentEvaluationSchema.nullable(),
});
export type AgentCard = z.infer<typeof agentCardSchema>;

const hostSchema = z
  .string()
  .trim()
  .regex(/^(\*\.)?[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*(:\d{1,5})?$/, {
    error: 'Use a host name such as api.example.com or *.example.com',
  });

export const capabilitiesInputSchema = z.object({
  tools: z.array(z.string().trim().min(1).max(100)).max(64).nullable(),
  data: z.object({
    topics: z.array(idSchema).max(64).nullable(),
    labels: z.array(dataLabelSchema),
    memory_namespaces: z.array(z.enum(MEMORY_NAMESPACES)).nullable(),
  }),
  egress_hosts: z.array(hostSchema).max(64).nullable(),
  counterparts: z
    .array(
      z
        .string()
        .trim()
        .regex(/^[a-z0-9_-]{1,64}$/, { error: 'Roles are lowercase slugs' }),
    )
    .max(64)
    .nullable(),
  delegation: z.object({
    max_depth: z.number({ error: 'Enter a number' }).int().min(0).max(8),
    max_fan_out: z.number({ error: 'Enter a number' }).int().min(0).max(32),
  }),
  models: z.array(z.string().trim().min(1).max(100)).max(64).nullable(),
});

export const agentInputSchema = z.object({
  name: z.string().trim().min(1, { error: 'Name is required' }).max(64),
  role: z
    .string()
    .trim()
    .min(1, { error: 'Role is required' })
    .max(64)
    .regex(/^[a-z0-9_-]+$/, { error: 'Use lowercase letters, digits, - or _' }),
  title: z.string().trim().max(120),
  model: z.string().trim().max(100),
  system_prompt: z.string().max(16 * 1024),
  reports_to: idSchema.nullable(),
  budget_tokens: z
    .number({ error: 'Enter a number' })
    .int({ error: 'Use a whole number' })
    .nonnegative({ error: 'Budget cannot be negative' }),
  runtime: agentRuntimeSchema,
  purpose: z.string().trim().max(2000),
  team_id: idSchema.nullable(),
  capabilities: capabilitiesInputSchema,
});
export type AgentInput = z.infer<typeof agentInputSchema>;

/** `Researcher` out of `agent://{workspace}/Researcher`, and a sub-agent's path under it. */
export function displayPrincipal(principal: string): string {
  const match = /^agent:\/\/[^/]+\/(.+)$/.exec(principal);
  return match?.[1] ?? principal;
}
