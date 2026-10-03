import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { executorSchema, nodeStatusSchema } from './graph';

export const RUN_STATUSES = ['queued', 'running', 'succeeded', 'failed', 'cancelled'] as const;
export const runStatusSchema = z.enum(RUN_STATUSES);
export type RunStatus = z.infer<typeof runStatusSchema>;

export const nodeRunSchema = z.object({
  node_id: idSchema,
  status: nodeStatusSchema,
  attempt: z.number().int().nonnegative(),
  executor: executorSchema,
  tokens_in: z.number().int().nonnegative(),
  tokens_out: z.number().int().nonnegative(),
  cached: z.boolean(),
  error: z.string().nullable(),
  started_at: nullableTimestampSchema,
  finished_at: nullableTimestampSchema,
  output_preview: z.string().nullable(),
});
export type NodeRun = z.infer<typeof nodeRunSchema>;

export const runSchema = z.object({
  id: idSchema,
  graph_id: idSchema,
  status: runStatusSchema,
  tokens_in: z.number().int().nonnegative(),
  tokens_out: z.number().int().nonnegative(),
  cost_usd: z.number().nonnegative(),
  started_at: nullableTimestampSchema,
  finished_at: nullableTimestampSchema,
  created_at: timestampSchema,
  node_runs: z.array(nodeRunSchema),
});
export type Run = z.infer<typeof runSchema>;

export const artifactSchema = z.object({
  id: idSchema,
  run_id: idSchema,
  node_id: idSchema,
  path: z.string(),
  size: z.number().int().nonnegative(),
  mime: z.string(),
  created_at: timestampSchema,
});
export type Artifact = z.infer<typeof artifactSchema>;

export interface StartRunBody {
  node_ids?: string[];
  max_concurrency?: number;
  force?: boolean;
}

export function isRunActive(status: RunStatus): boolean {
  return status === 'queued' || status === 'running';
}
