import { z } from 'zod';

import { dataLabelSchema } from './agent';
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
  /** Classification: only agents cleared for it receive the document's passages. */
  label: dataLabelSchema,
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
  topic_id: idSchema.nullable(),
  /** The document's classification, and the topic's when the passage has one. */
  label: dataLabelSchema,
  topic_label: dataLabelSchema.nullable(),
  score: z.number(),
});
export type Passage = z.infer<typeof passageSchema>;

/** A topic of the workspace's documents, found by clustering their passages. */
export const topicSchema = z.object({
  id: idSchema,
  /** The words that set the topic apart. */
  label: z.string(),
  terms: z.array(z.string()),
  chunk_count: z.number().int().nonnegative(),
  /** Classification of every passage under the topic, on top of its document's. */
  classification: dataLabelSchema,
});
export type Topic = z.infer<typeof topicSchema>;

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
