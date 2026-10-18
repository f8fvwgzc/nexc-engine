import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { FRESH } from '@/lib/freshness';
import { qk } from '@/lib/query-keys';
import {
  mcpCheckSchema,
  mcpServerSchema,
  toMcpServerBody,
  type McpServerInput,
} from '@/schemas/mcp';

/** The MCP servers of a workspace, by name. */
export const mcpServersQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.mcp.list(workspaceId),
    staleTime: FRESH.setup,
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/mcp-servers`, z.array(mcpServerSchema), {
        signal,
      }),
  });

/** Registers a server (admins); the backend connects to it at once. */
export function registerMcpServer(workspaceId: string, input: McpServerInput) {
  return apiRequest(`/workspaces/${workspaceId}/mcp-servers`, mcpServerSchema, {
    method: 'POST',
    body: toMcpServerBody(input),
  });
}

/** Connects to a server again and records the tools it lists (admins). */
export function checkMcpServer(workspaceId: string, serverId: string) {
  return apiRequest(`/workspaces/${workspaceId}/mcp-servers/${serverId}/check`, mcpCheckSchema, {
    method: 'POST',
  });
}

/** Removes a server (admins). */
export function removeMcpServer(workspaceId: string, serverId: string) {
  return apiSend(`/workspaces/${workspaceId}/mcp-servers/${serverId}`, { method: 'DELETE' });
}
