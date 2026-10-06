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

/** The kinds of things a workspace holds and the ties between them, counted. */
export const workspaceMapSchema = z.object({
  entities: z.array(z.object({ key: z.string(), label: z.string(), count: z.number().int() })),
  relations: z.array(
    z.object({ from: z.string(), to: z.string(), label: z.string(), count: z.number().int() }),
  ),
});
export type WorkspaceMap = z.infer<typeof workspaceMapSchema>;

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
  embedding_model: z.string(),
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
