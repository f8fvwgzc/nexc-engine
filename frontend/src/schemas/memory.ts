import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const MEMORY_KINDS = ['fact', 'experience', 'observation', 'preference'] as const;
export const memoryKindSchema = z.enum(MEMORY_KINDS);
export type MemoryKind = z.infer<typeof memoryKindSchema>;

export const memorySchema = z.object({
  id: idSchema,
  scope: z.enum(['user', 'graph', 'node']),
  graph_id: idSchema.nullable(),
  node_id: idSchema.nullable(),
  kind: memoryKindSchema,
  content: z.string(),
  importance: z.number(),
  access_count: z.number().int().nonnegative(),
  score: z.number().nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type Memory = z.infer<typeof memorySchema>;

export interface MemoryQuery {
  /** The workspace whose memory to read. */
  workspace_id?: string;
  q?: string;
  graph_id?: string;
  limit?: number;
}
