import { z } from 'zod';

import { idSchema, nullableTimestampSchema, timestampSchema } from './common';

/** What a span records (CONTRACT §4, the trace spine). */
export const SPAN_KINDS = [
  'llm_call',
  'tool_call',
  'spawn',
  'data_read',
  'egress',
  'policy',
  'approval',
] as const;
export const spanKindSchema = z.enum(SPAN_KINDS);
export type SpanKind = z.infer<typeof spanKindSchema>;

export const spanStatusSchema = z.enum(['ok', 'error']);
export type SpanStatus = z.infer<typeof spanStatusSchema>;

const hashSchema = z.string().regex(/^[0-9a-f]{64}$/);

/** One action of an agent in a run; append-only and hash-chained per run. */
export const spanSchema = z.object({
  id: idSchema,
  run_id: idSchema,
  node_id: idSchema,
  agent_id: idSchema.nullable(),
  principal: z.string(),
  // The runtime names parents; any id it reported is kept, so this is not checked as a UUID.
  parent_id: z.string().nullable(),
  kind: spanKindSchema,
  name: z.string(),
  started_at: timestampSchema,
  ended_at: nullableTimestampSchema,
  status: spanStatusSchema,
  attributes: z.record(z.string(), z.unknown()),
  seq: z.number().int().nonnegative(),
  prev_hash: hashSchema,
  hash: hashSchema,
});
export type Span = z.infer<typeof spanSchema>;

export const tracePageSchema = z.object({
  spans: z.array(spanSchema),
  next: idSchema.nullable(),
});
export type TracePage = z.infer<typeof tracePageSchema>;

export const traceVerificationSchema = z.object({
  ok: z.boolean(),
  spans: z.number().int().nonnegative(),
  hash: hashSchema.nullable(),
  sealed_hash: hashSchema.nullable(),
  broken: z
    .object({ span_id: idSchema, seq: z.number().int().nonnegative(), reason: z.string() })
    .nullable(),
});
export type TraceVerification = z.infer<typeof traceVerificationSchema>;
