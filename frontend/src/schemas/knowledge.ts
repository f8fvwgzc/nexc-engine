import { z } from 'zod';

import { idSchema, timestampSchema } from './common';

export const documentStatusSchema = z.enum(['pending', 'parsing', 'embedding', 'ready', 'failed']);
export type DocumentStatus = z.infer<typeof documentStatusSchema>;

/** An uploaded document and how far it is on its way into the knowledge base. */
export const documentSchema = z.object({
  id: idSchema,
  workspace_id: idSchema,
  name: z.string(),
  size_bytes: z.number().int().nonnegative(),
  status: documentStatusSchema,
  /** Why it failed; empty otherwise. */
  error: z.string(),
  page_count: z.number().int().nullable(),
  chunk_count: z.number().int().nonnegative(),
  uploaded_by: idSchema.nullable(),
  created_at: timestampSchema,
  updated_at: timestampSchema,
});
export type KnowledgeDocument = z.infer<typeof documentSchema>;

/** A passage found by a search, with where it comes from. */
export const passageSchema = z.object({
  chunk_id: idSchema,
  document_id: idSchema,
  document_name: z.string(),
  page: z.number().int().nullable(),
  section_path: z.string(),
  kind: z.enum(['text', 'table']),
  content: z.string(),
  score: z.number(),
});
export type Passage = z.infer<typeof passageSchema>;

/** How the workspace embeds and uses its documents. The key itself is never returned. */
export const knowledgeSettingsSchema = z.object({
  embed_base_url: z.string().nullable(),
  embed_model: z.string(),
  embed_dims: z.number().int().nullable(),
  has_api_key: z.boolean(),
  key_hint: z.string().nullable(),
  /** False for the built-in embedding, which matches words rather than meaning. */
  semantic: z.boolean(),
  passages: z.number().int(),
  budget_chars: z.number().int(),
  use_in_nodes: z.boolean(),
  use_in_plan: z.boolean(),
});
export type KnowledgeSettings = z.infer<typeof knowledgeSettingsSchema>;

export interface KnowledgeSettingsInput {
  embed_base_url: string | null;
  embed_model: string | null;
  embed_dims: number | null;
  /** Left out keeps the stored key; an empty string removes it. */
  api_key?: string;
  passages: number;
  budget_chars: number;
  use_in_nodes: boolean;
  use_in_plan: boolean;
}
