import { z } from 'zod';

export const llmProviderSchema = z.enum(['anthropic', 'openai_compatible', 'claude_code', 'demo']);
export type LlmProvider = z.infer<typeof llmProviderSchema>;

export const llmSettingsSchema = z.object({
  provider: llmProviderSchema,
  model: z.string(),
  base_url: z.string().nullable(),
  has_api_key: z.boolean(),
  key_hint: z.string().nullable(),
  source: z.enum(['user', 'server', 'none']),
});
export type LlmSettings = z.infer<typeof llmSettingsSchema>;

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
