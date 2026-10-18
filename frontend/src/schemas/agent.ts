import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { evidenceVerdictSchema } from './evidence';
import { spanSchema } from './trace';

export const AGENT_STATUSES = ['active', 'paused', 'over_budget', 'quarantined'] as const;
export const agentStatusSchema = z.enum(AGENT_STATUSES);
export type AgentStatus = z.infer<typeof agentStatusSchema>;

export const agentRuntimeSchema = z.enum(['python', 'builtin', 'external']);
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
  /** Published as an A2A agent (CONTRACT §10): its card is served, counterparts may send it tasks. */
  a2a_exposed: z.boolean(),
});
export type Agent = z.infer<typeof agentSchema>;

/** The states of an A2A task, from the run that answers it. */
export const A2A_TASK_STATES = ['submitted', 'working', 'completed', 'canceled', 'failed'] as const;
export const a2aTaskStateSchema = z.enum(A2A_TASK_STATES);
export type A2aTaskState = z.infer<typeof a2aTaskStateSchema>;

/** A task an exposed agent received over A2A, as the console lists it (CONTRACT §4). */
export const a2aTaskSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  agent_id: idSchema,
  caller_agent_id: idSchema.nullable(),
  caller_principal: z.string(),
  run_id: idSchema,
  node_id: idSchema,
  session_id: z.string().nullable(),
  /** A2A hops before the task (0 for a counterpart's own call). */
  hop: z.number().int().nonnegative(),
  chain: z.array(z.string()),
  /** The rule that refused it, when the firewall did. */
  refused_rule: z.string().nullable(),
  refused_reason: z.string().nullable(),
  title: z.string(),
  /** One of `A2A_TASK_STATES` (kept open: the server may add one). */
  state: z.string(),
  created_at: timestampSchema,
});
export type A2aTask = z.infer<typeof a2aTaskSchema>;

/** An API token of an external agent (CONTRACT §8): what is kept of it, never its value. */
export const agentApiTokenSchema = z.object({
  id: idSchema,
  agent_id: idSchema,
  /** The token's first characters, to tell tokens apart. */
  prefix: z.string(),
  name: z.string(),
  created_by: idSchema.nullable(),
  created_at: timestampSchema,
  last_used_at: nullableTimestampSchema,
  revoked_at: nullableTimestampSchema,
});
export type AgentApiToken = z.infer<typeof agentApiTokenSchema>;

/** A token just issued: `secret` (`nexc_agt_…`) is given out this once. */
export const issuedAgentApiTokenSchema = z.object({
  token: agentApiTokenSchema,
  secret: z.string(),
});
export type IssuedAgentApiToken = z.infer<typeof issuedAgentApiTokenSchema>;

/** What a token may be called (`TOKEN_NAME_MAX` in the backend). */
export const TOKEN_NAME_MAX = 100;

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

/** The verdict on one of an agent's outputs (AGENTSECOPS §2). */
export const agentOutputEvidenceSchema = z.object({
  run_id: idSchema,
  graph_id: idSchema,
  node_id: idSchema,
  verdict: evidenceVerdictSchema,
  coverage: z.number().min(0).max(1),
  finished_at: nullableTimestampSchema,
});
export type AgentOutputEvidence = z.infer<typeof agentOutputEvidenceSchema>;

/** The verdicts on an agent's last outputs, newest first. */
export const agentEvidenceSchema = z.object({
  verified: z.number().int().nonnegative(),
  partial: z.number().int().nonnegative(),
  unverified: z.number().int().nonnegative(),
  recent: z.array(agentOutputEvidenceSchema),
});
export type AgentEvidence = z.infer<typeof agentEvidenceSchema>;

/** The agent's ID card: who answers for it, its team, what it has left and did last. */
export const agentCardSchema = z.object({
  agent: agentSchema,
  owner: z.object({ id: idSchema, name: z.string() }).nullable(),
  team: z.object({ id: idSchema, name: z.string(), key: z.string() }).nullable(),
  budget_left: z.number().int().nonnegative().nullable(),
  recent_spans: z.array(spanSchema),
  /** The score of an agent made from a role template; `null` otherwise. */
  evaluation: agentEvaluationSchema.nullable(),
  /** The verdicts on the agent's last twenty outputs across the runs the caller may open. */
  evidence: agentEvidenceSchema,
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
