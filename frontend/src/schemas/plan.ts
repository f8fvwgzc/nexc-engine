import { z } from 'zod';

import { idSchema, timestampSchema } from './common';
import { executorSchema, nodeKindSchema } from './graph';

export const planStatusSchema = z.enum(['streaming', 'ready', 'failed', 'applied']);
export type PlanStatus = z.infer<typeof planStatusSchema>;

export const proposedNodeSchema = z.object({
  ref: z.string().min(1),
  existing_id: idSchema.nullable(),
  title: z.string(),
  content: z.string(),
  kind: nodeKindSchema,
  agent_role: z.string(),
  executor: executorSchema,
  tags: z.array(z.string()),
});
export type ProposedNode = z.infer<typeof proposedNodeSchema>;

export const proposedEdgeSchema = z.object({
  source_ref: z.string().min(1),
  target_ref: z.string().min(1),
});
export type ProposedEdge = z.infer<typeof proposedEdgeSchema>;

export const planSchema = z.object({
  id: idSchema,
  graph_id: idSchema,
  status: planStatusSchema,
  summary: z.string(),
  nodes: z.array(proposedNodeSchema),
  edges: z.array(proposedEdgeSchema),
  error: z.string().nullable(),
  created_at: timestampSchema,
});
export type Plan = z.infer<typeof planSchema>;
