import { z } from 'zod';

import { dataLabelSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { executorSchema } from './graph';
import { runStatusSchema } from './run';
import { spanKindSchema } from './trace';

/** Where a node or an agent subtree stands (CONTRACT §4, sessions). */
export const SESSION_STATUSES = [
  'queued',
  'running',
  'waiting_approval',
  'succeeded',
  'failed',
  'cancelled',
  'skipped',
] as const;
export const sessionStatusSchema = z.enum(SESSION_STATUSES);
export type SessionStatus = z.infer<typeof sessionStatusSchema>;

/** What a subtree spent and did. */
export const rollupSchema = z.object({
  spans: z.number().int().nonnegative(),
  llm_calls: z.number().int().nonnegative(),
  tool_calls: z.number().int().nonnegative(),
  denials: z.number().int().nonnegative(),
  tokens_in: z.number().int().nonnegative(),
  tokens_out: z.number().int().nonnegative(),
  tokens_cached: z.number().int().nonnegative(),
  cost_usd: z.number().nonnegative(),
});
export type Rollup = z.infer<typeof rollupSchema>;

/** One agent of a node with its sub-agents under it. */
export interface SessionAgent {
  principal: string;
  agent_id: string | null;
  role: string | null;
  depth: number;
  spawn_span_id: string | null;
  parent_principal: string | null;
  status: SessionStatus;
  started_at: string | null;
  ended_at: string | null;
  duration_ms: number | null;
  own: Rollup;
  total: Rollup;
  span_ids: string[];
  cancellable: boolean;
  children: SessionAgent[];
}

export const sessionAgentSchema: z.ZodType<SessionAgent> = z.lazy(() =>
  z.object({
    principal: z.string(),
    agent_id: idSchema.nullable(),
    role: z.string().nullable(),
    depth: z.number().int().nonnegative(),
    spawn_span_id: idSchema.nullable(),
    parent_principal: z.string().nullable(),
    status: sessionStatusSchema,
    started_at: nullableTimestampSchema,
    ended_at: nullableTimestampSchema,
    duration_ms: z.number().int().nonnegative().nullable(),
    own: rollupSchema,
    total: rollupSchema,
    span_ids: z.array(idSchema),
    cancellable: z.boolean(),
    children: z.array(sessionAgentSchema),
  }),
);

export const sessionNodeSchema = z.object({
  node_id: idSchema,
  title: z.string(),
  executor: executorSchema,
  status: sessionStatusSchema,
  attempt: z.number().int().nonnegative(),
  cached: z.boolean(),
  error: z.string().nullable(),
  started_at: nullableTimestampSchema,
  ended_at: nullableTimestampSchema,
  duration_ms: z.number().int().nonnegative().nullable(),
  rollup: rollupSchema,
  context_span_ids: z.array(idSchema),
  agents: z.array(sessionAgentSchema),
});
export type SessionNode = z.infer<typeof sessionNodeSchema>;

export const modelRollupSchema = z.object({
  provider: z.string(),
  model: z.string(),
  llm_calls: z.number().int().nonnegative(),
  tokens_in: z.number().int().nonnegative(),
  tokens_out: z.number().int().nonnegative(),
  tokens_cached: z.number().int().nonnegative(),
  cost_usd: z.number().nonnegative(),
});
export type ModelRollup = z.infer<typeof modelRollupSchema>;

/** A run as a tree: nodes, the agent each ran as, its sub-agents. */
export const sessionSchema = z.object({
  run_id: idSchema,
  graph_id: idSchema,
  graph_name: z.string(),
  workspace_id: idSchema.nullable(),
  status: runStatusSchema,
  created_at: timestampSchema,
  started_at: nullableTimestampSchema,
  finished_at: nullableTimestampSchema,
  duration_ms: z.number().int().nonnegative().nullable(),
  rollup: rollupSchema,
  by_model: z.array(modelRollupSchema),
  /** Prompts and completions were being recorded when the run's nodes started. */
  recording: z.boolean(),
  nodes: z.array(sessionNodeSchema),
});
export type Session = z.infer<typeof sessionSchema>;

/** One row of a workspace's session list. */
export const sessionSummarySchema = z.object({
  run_id: idSchema,
  graph_id: idSchema,
  graph_name: z.string(),
  status: runStatusSchema,
  created_at: timestampSchema,
  started_at: nullableTimestampSchema,
  finished_at: nullableTimestampSchema,
  duration_ms: z.number().int().nonnegative().nullable(),
  nodes: z.number().int().nonnegative(),
  agents: z.array(z.object({ id: idSchema, name: z.string() })),
  rollup: rollupSchema,
});
export type SessionSummary = z.infer<typeof sessionSummarySchema>;

export const sessionPageSchema = z.object({
  sessions: z.array(sessionSummarySchema),
  next: idSchema.nullable(),
});
export type SessionPage = z.infer<typeof sessionPageSchema>;

/** What the list narrows by; every field optional. */
export interface SessionFilter {
  agent?: string;
  status?: z.infer<typeof runStatusSchema>;
  from?: string;
  to?: string;
}

/** What stopping a sub-agent recorded. */
export const agentCancelSchema = z.object({
  run_id: idSchema,
  node_id: idSchema,
  spawn_span_id: idSchema,
  principal: z.string(),
  requested_at: timestampSchema,
  delivered: z.boolean(),
});
export type AgentCancel = z.infer<typeof agentCancelSchema>;

/* ---------- what a span said (recording) ---------- */

export const PAYLOAD_KINDS = ['prompt', 'completion', 'tool_input', 'tool_output'] as const;
export const payloadKindSchema = z.enum(PAYLOAD_KINDS);
export type PayloadKind = z.infer<typeof payloadKindSchema>;

export const spanPayloadSchema = z.object({
  kind: payloadKindSchema,
  content: z.string(),
  size: z.number().int().nonnegative(),
  redactions: z.number().int().nonnegative(),
  created_at: timestampSchema,
});
export type SpanPayload = z.infer<typeof spanPayloadSchema>;

export const spanPayloadsSchema = z.object({
  span_id: idSchema,
  run_id: idSchema,
  node_id: idSchema,
  kind: spanKindSchema,
  labels: z.array(dataLabelSchema),
  payloads: z.array(spanPayloadSchema),
});
export type SpanPayloads = z.infer<typeof spanPayloadsSchema>;

export const PAYLOAD_KIND_LABEL: Record<PayloadKind, string> = {
  prompt: 'Prompt',
  completion: 'Completion',
  tool_input: 'Tool input',
  tool_output: 'Tool output',
};
