import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { agentSchema, type AgentInput } from '@/schemas/agent';

/** The agents of a workspace (the caller's first workspace when none is given). */
export const agentsQuery = (workspaceId?: string) =>
  queryOptions({
    queryKey: qk.agents.list(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(
        workspaceId ? `/agents?workspace_id=${workspaceId}` : '/agents',
        z.array(agentSchema),
        { signal },
      ),
  });

export function createAgent(body: AgentInput & { workspace_id?: string }) {
  return apiRequest('/agents', agentSchema, { method: 'POST', body });
}

export function updateAgent(agentId: string, body: AgentInput) {
  return apiRequest(`/agents/${agentId}`, agentSchema, { method: 'PATCH', body });
}

export function deleteAgent(agentId: string) {
  return apiSend(`/agents/${agentId}`, { method: 'DELETE' });
}
