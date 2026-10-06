import { queryOptions } from '@tanstack/react-query';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { llmSettingsSchema, modelCatalogSchema, type LlmProvider } from '@/schemas/settings';

const scoped = (path: string, workspaceId?: string) =>
  workspaceId ? `${path}?workspace_id=${workspaceId}` : path;

/**
 * The settings in effect for the caller: their own account, else the credential of the
 * workspace they are working in, else the server default.
 */
export const llmSettingsQuery = (workspaceId?: string) =>
  queryOptions({
    queryKey: qk.settings.effective(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(scoped('/settings/llm', workspaceId), llmSettingsSchema, { signal }),
  });

/** The credential a workspace holds; null when it has none. */
export const workspaceLlmQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.settings.workspace(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/llm`, llmSettingsSchema.nullable(), { signal }),
  });

/** The models the effective provider offers right now, newest first. */
export const llmModelsQuery = (workspaceId?: string) =>
  queryOptions({
    queryKey: qk.settings.models(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(scoped('/settings/llm/models', workspaceId), modelCatalogSchema, { signal }),
    staleTime: 10 * 60_000,
    retry: false,
  });

export interface UpdateLlmSettingsBody {
  provider: LlmProvider;
  model: string;
  base_url?: string | null;
  /** Omit to keep the stored key, `""` deletes it. Write-only — never returned. */
  api_key?: string;
}

/** Connects (or updates) the caller's own account. */
export function updateLlmSettings(body: UpdateLlmSettingsBody) {
  return apiRequest('/settings/llm', llmSettingsSchema, { method: 'PUT', body });
}

/** Disconnects the caller's own account; the workspace's credential applies again. */
export function disconnectLlm() {
  return apiSend('/settings/llm', { method: 'DELETE' });
}

export function updateWorkspaceLlm(workspaceId: string, body: UpdateLlmSettingsBody) {
  return apiRequest(`/workspaces/${workspaceId}/llm`, llmSettingsSchema, { method: 'PUT', body });
}

export function removeWorkspaceLlm(workspaceId: string) {
  return apiSend(`/workspaces/${workspaceId}/llm`, { method: 'DELETE' });
}
