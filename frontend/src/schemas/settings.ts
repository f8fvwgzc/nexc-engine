import { z } from 'zod';

export const llmProviderSchema = z.enum(['anthropic', 'openai_compatible', 'claude_code', 'demo']);
export type LlmProvider = z.infer<typeof llmProviderSchema>;

export const llmSettingsSchema = z.object({
  provider: llmProviderSchema,
  model: z.string(),
  base_url: z.string().nullable(),
  has_api_key: z.boolean(),
  key_hint: z.string().nullable(),
  /** Where the API key comes from. */
  source: z.enum(['user', 'workspace', 'server', 'none']),
  /** Whose configuration is in effect: the user's own account, the workspace's, or the server's. */
  scope: z.enum(['user', 'workspace', 'server']),
});
export type LlmSettings = z.infer<typeof llmSettingsSchema>;

/** One model a provider currently offers. */
export const modelInfoSchema = z.object({
  id: z.string(),
  name: z.string(),
  released_at: z.string().nullable(),
  /** Released within roughly the last four months. */
  recent: z.boolean(),
});
export type ModelInfo = z.infer<typeof modelInfoSchema>;

/** The provider's own answer to "which models do you offer?" — never a built-in list. */
export const modelCatalogSchema = z.object({
  provider: llmProviderSchema,
  models: z.array(modelInfoSchema),
  note: z.string().nullable(),
});
export type ModelCatalog = z.infer<typeof modelCatalogSchema>;

/** PUT /settings/llm body. `api_key: ""` deletes the stored key; omit it to keep the current one. */
export const llmSettingsInputSchema = z.object({
  provider: llmProviderSchema,
  model: z.string().trim().min(1, { error: 'Model is required' }).max(200),
  base_url: z.union([
    z.url({ error: 'Enter a full URL, e.g. https://api.example.com/v1' }),
    z.literal(''),
  ]),
  api_key: z.string().max(500),
});
export type LlmSettingsInput = z.infer<typeof llmSettingsInputSchema>;
