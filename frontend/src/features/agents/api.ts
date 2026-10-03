import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { agentSchema, type AgentInput } from '@/schemas/agent';

export const agentsQuery = () =>
  queryOptions({
    queryKey: qk.agents.all,
    queryFn: ({ signal }) => apiRequest('/agents', z.array(agentSchema), { signal }),
  });

export function createAgent(body: AgentInput) {
  return apiRequest('/agents', agentSchema, { method: 'POST', body });
}

export function updateAgent(agentId: string, body: AgentInput) {
  return apiRequest(`/agents/${agentId}`, agentSchema, { method: 'PATCH', body });
}

export function deleteAgent(agentId: string) {
  return apiSend(`/agents/${agentId}`, { method: 'DELETE' });
}
