import { z } from 'zod';

import { dataLabelSchema } from './agent';
import { idSchema, nullableTimestampSchema, timestampSchema } from './common';
import { executorSchema, nodeStatusSchema } from './graph';
import { policyVersionsSchema } from './policy';

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
  /** The policy versions that governed the node, by level. */
  policy_versions: policyVersionsSchema,
  /** The classifications of the data the node was given. */
  context_labels: z.array(dataLabelSchema),
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
  /** Hex SHA-256 sealing the run's trace once it finished; null while it runs. */
  trace_hash: z.string().nullable(),
  /** Actions the runtime reported that the run's policy did not allow. */
  policy_mismatches: z.number().int().nonnegative(),
  /** The workspace policy version that governed the run. */
  policy_versions: policyVersionsSchema,
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
  /** The DLP classifier kept the file in; downloadable only once released. */
  quarantined_at: nullableTimestampSchema,
  quarantine_reason: z.string(),
  released_at: nullableTimestampSchema,
});
export type Artifact = z.infer<typeof artifactSchema>;

/** Whether the file may be downloaded. */
export function isDownloadable(artifact: Artifact): boolean {
  return artifact.quarantined_at === null || artifact.released_at !== null;
}

export interface StartRunBody {
  node_ids?: string[];
  max_concurrency?: number;
  force?: boolean;
}

export function isRunActive(status: RunStatus): boolean {
  return status === 'queued' || status === 'running';
}
