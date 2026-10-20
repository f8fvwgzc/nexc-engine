import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

/** One thing that happened in a workspace. `kind` is open: new kinds may appear. */
export const timelineEntrySchema = z.object({
  at: timestampSchema,
  kind: z.string(),
  actor: z.string().nullable(),
  entity_type: z.string(),
  entity_id: idSchema.nullable(),
  title: z.string(),
  detail: z.string(),
});
export type TimelineEntry = z.infer<typeof timelineEntrySchema>;

export const timelineDaySchema = z.object({ day: z.string(), events: z.number().int() });
export type TimelineDay = z.infer<typeof timelineDaySchema>;

/** A day of the timeline, told in a few lines by the workspace's model. */
export const daySummarySchema = z.object({
  day: z.string(),
  headline: z.string(),
  highlights: z.array(z.string()),
  attention: z.array(z.string()),
  event_count: z.number().int(),
  /** More has happened on the day since it was written. */
  stale: z.boolean(),
  model: z.string(),
  created_by: z.string().nullable(),
  created_at: timestampSchema,
});
export type DaySummary = z.infer<typeof daySummarySchema>;

/** What the server runs on (server administrators only). */
export const infrastructureSchema = z.object({
  database: z.object({
    location: z.string(),
    version: z.string(),
    size_bytes: z.number().int(),
    pgvector: z.string().nullable(),
    migrations_applied: z.number().int(),
    migrations_known: z.number().int(),
  }),
  runtime_url: z.string(),
  runtime_reachable: z.boolean(),
  /** `grpc` or `ndjson`: the wire the backend speaks to the runtime. */
  runtime_protocol: z.string(),
  runtime_grpc: z.object({
    enabled: z.boolean(),
    ok: z.boolean(),
    url: z.string().nullable(),
    detail: z.string().nullable(),
  }),
  /** The Agent Firewall API: where it listens and the key it signs with. */
  firewall: z.object({
    enabled: z.boolean(),
    addr: z.string().nullable(),
    public_key: z.string(),
    open_sessions: z.number().int(),
  }),
  /** The OpenTelemetry export of the trace. */
  otlp: z.object({
    enabled: z.boolean(),
    endpoint: z.string().nullable(),
    reachable: z.boolean(),
    spans_exported: z.number().int(),
    behind: z.number().int(),
    last_exported_at: z.string().nullable(),
    last_error: z.string().nullable(),
  }),
  gateway: z.object({
    calls_per_minute: z.number().int(),
    timeout_secs: z.number().int(),
    max_response_bytes: z.number().int(),
  }),
  embedding_model: z.string(),
  /** The onto-intelli the workspaces' ontology lives in; disabled when none is configured. */
  ontology: z.object({
    enabled: z.boolean(),
    ok: z.boolean(),
    url: z.string().nullable(),
    detail: z.string().nullable(),
  }),
  queue: z.string(),
  cache: z.string(),
});
export type Infrastructure = z.infer<typeof infrastructureSchema>;

export const connectionCheckSchema = z.object({
  reachable: z.boolean(),
  detail: z.string(),
  pgvector_available: z.boolean().nullable(),
  migrations_applied: z.number().int().nullable(),
});
export type ConnectionCheck = z.infer<typeof connectionCheckSchema>;

/** One endpoint's latency over the last five minutes of one server instance. */
export const latencyRowSchema = z.object({
  method: z.string(),
  route: z.string(),
  count: z.number().int(),
  p50_ms: z.number(),
  p99_ms: z.number(),
  max_ms: z.number(),
});
export type LatencyRow = z.infer<typeof latencyRowSchema>;

export const latencyReportSchema = z.object({
  window_secs: z.number().int(),
  rows: z.array(latencyRowSchema),
});
export type LatencyReport = z.infer<typeof latencyReportSchema>;
