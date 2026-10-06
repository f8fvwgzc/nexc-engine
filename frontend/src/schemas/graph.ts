import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const NODE_STATUSES = [
  'idle',
  'queued',
  'running',
  'succeeded',
  'failed',
  'skipped',
  'cancelled',
] as const;
export const nodeStatusSchema = z.enum(NODE_STATUSES);
export type NodeStatus = z.infer<typeof nodeStatusSchema>;

export const EXECUTORS = ['llm', 'agent', 'symphony'] as const;
export const executorSchema = z.enum(EXECUTORS);
export type Executor = z.infer<typeof executorSchema>;

/** A node type of a graph's ontology. Node `kind`s are keys of these. */
export const nodeTypeSchema = z.object({
  key: z.string().min(1),
  label: z.string(),
  description: z.string().default(''),
  color: z.string().default(''),
  icon: z.string().default(''),
  default_role: z.string().default(''),
  default_executor: executorSchema.default('llm'),
  stage: z.number().int().default(0),
  produces_artifact: z.boolean().default(false),
  allow_code_exec: z.boolean().default(false),
});
export type NodeType = z.infer<typeof nodeTypeSchema>;

/** A relation type of a graph's ontology. Edge `kind`s are keys of these. */
export const relationTypeSchema = z.object({
  key: z.string().min(1),
  label: z.string(),
  description: z.string().default(''),
  /** The source must finish before the target runs (blocking relations form the DAG). */
  blocking: z.boolean().default(false),
});
export type RelationType = z.infer<typeof relationTypeSchema>;

/** The vocabulary a graph is built from. It is data owned by the graph, not a fixed list. */
export const ontologySchema = z.object({
  node_types: z.array(nodeTypeSchema).default([]),
  relation_types: z.array(relationTypeSchema).default([]),
});
export type Ontology = z.infer<typeof ontologySchema>;

export const EMPTY_ONTOLOGY: Ontology = { node_types: [], relation_types: [] };

export const graphNodeSchema = z.object({
  id: idSchema,
  graph_id: idSchema,
  title: z.string(),
  content: z.string(),
  /** Key of a node type of the graph's ontology. */
  kind: z.string(),
  tags: z.array(z.string()),
  x: z.number(),
  y: z.number(),
  status: nodeStatusSchema,
  agent_role: z.string().nullable(),
  executor: executorSchema,
  output: z.string().nullable(),
  origin: z.enum(['user', 'plan']),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type GraphNode = z.infer<typeof graphNodeSchema>;

/** `kind` is a relation type key; `blocking`: source must finish before target runs. */
export const graphEdgeSchema = z.object({
  id: idSchema,
  graph_id: idSchema,
  source: idSchema,
  target: idSchema,
  kind: z.string(),
  blocking: z.boolean(),
  /** Why the two nodes are related this way. */
  reason: z.string(),
  origin: z.enum(['user', 'auto', 'plan']),
  weight: z.number(),
});
export type GraphEdge = z.infer<typeof graphEdgeSchema>;

export const graphSummarySchema = z.object({
  id: idSchema,
  workspace_id: idSchema.nullable(),
  team_id: idSchema.nullable(),
  name: z.string(),
  description: z.string(),
  node_count: z.number().int().nonnegative(),
  edge_count: z.number().int().nonnegative(),
  updated_at: timestampSchema,
});
export type GraphSummary = z.infer<typeof graphSummarySchema>;

export const graphSchema = z.object({
  id: idSchema,
  /** The workspace the graph belongs to. */
  workspace_id: idSchema.nullable(),
  /** The team it belongs to; null for a graph of the whole workspace. */
  team_id: idSchema.nullable(),
  name: z.string(),
  description: z.string(),
  goal: z.string(),
  version: z.number().int(),
  ontology: ontologySchema,
  nodes: z.array(graphNodeSchema),
  edges: z.array(graphEdgeSchema),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type Graph = z.infer<typeof graphSchema>;

export const edgeSuggestionSchema = z.object({
  source: idSchema,
  target: idSchema,
  score: z.number(),
  reason: z.string(),
});
export type EdgeSuggestion = z.infer<typeof edgeSuggestionSchema>;

export const graphAnalysisSchema = z.object({
  topo_order: z.array(idSchema),
  levels: z.array(z.array(idSchema)),
  critical_path: z.array(idSchema),
  cycles: z.array(z.array(idSchema)),
  components: z.array(z.array(idSchema)),
});
export type GraphAnalysis = z.infer<typeof graphAnalysisSchema>;

/* ---------- request bodies / form schemas ---------- */

export const TITLE_MAX = 200;
export const CONTENT_MAX = 64 * 1024;

export const graphInputSchema = z.object({
  name: z.string().trim().min(1, { error: 'Give the graph a name' }).max(TITLE_MAX),
  description: z.string().max(2000),
  goal: z.string().max(4000),
});
export type GraphInput = z.infer<typeof graphInputSchema>;

/** Node editor form. `tags` is edited as comma-separated text (see `parseTags`). */
export const nodeFormSchema = z.object({
  title: z
    .string()
    .trim()
    .min(1, { error: 'Title is required' })
    .max(TITLE_MAX, { error: `Title must be at most ${TITLE_MAX} characters` }),
  content: z.string().refine((v) => new TextEncoder().encode(v).length <= CONTENT_MAX, {
    error: 'Content must be at most 64 KiB',
  }),
  kind: z.string().min(1, { error: 'Pick a kind' }),
  executor: executorSchema,
  agent_role: z.string().trim().max(100, { error: 'Role must be at most 100 characters' }),
  tags: z
    .string()
    .max(1000)
    .refine((v) => parseTags(v).every((t) => t.length <= 50), {
      error: 'Each tag must be at most 50 characters',
    }),
});
export type NodeFormValues = z.infer<typeof nodeFormSchema>;

export function parseTags(text: string): string[] {
  return [
    ...new Set(
      text
        .split(',')
        .map((t) => t.trim())
        .filter(Boolean),
    ),
  ];
}

export interface CreateNodeBody {
  title: string;
  content?: string;
  /** Omit to get the default type of the graph's ontology. */
  kind?: string;
  tags?: string[];
  x?: number;
  y?: number;
  executor?: Executor;
  agent_role?: string | null;
}

export type UpdateNodeBody = Partial<
  Pick<GraphNode, 'title' | 'content' | 'kind' | 'tags' | 'x' | 'y' | 'executor' | 'agent_role'>
>;

export interface CreateEdgeBody {
  source: string;
  target: string;
  /** Omit to get the dependency relation of the graph's ontology. */
  kind?: string;
  reason?: string;
}
