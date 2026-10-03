import { queryOptions } from '@tanstack/react-query';

import { apiRequest } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { llmSettingsSchema, type LlmProvider } from '@/schemas/settings';

export const llmSettingsQuery = () =>
  queryOptions({
    queryKey: qk.settings.llm,
    queryFn: ({ signal }) => apiRequest('/settings/llm', llmSettingsSchema, { signal }),
  });

export interface UpdateLlmSettingsBody {
  provider: LlmProvider;
  model: string;
  base_url?: string | null;
  /** Omit to keep the stored key, `""` deletes it. Write-only — never returned. */
  api_key?: string;
}

export function updateLlmSettings(body: UpdateLlmSettingsBody) {
  return apiRequest('/settings/llm', llmSettingsSchema, { method: 'PUT', body });
}
