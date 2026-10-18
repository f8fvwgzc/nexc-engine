import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  a2aTaskSchema,
  agentApiTokenSchema,
  agentCardSchema,
  agentSchema,
  issuedAgentApiTokenSchema,
  type AgentInput,
} from '@/schemas/agent';
import { spanSchema } from '@/schemas/trace';

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

/** What an agent did last, across runs the caller may open: newest first. */
export const agentActivityQuery = (agentId: string, limit = 5) =>
  queryOptions({
    queryKey: [...qk.agents.activity(agentId), limit],
    queryFn: ({ signal }) =>
      apiRequest(`/agents/${agentId}/activity`, z.array(spanSchema), {
        signal,
        query: { limit },
      }),
    staleTime: 15_000,
  });

/** The agent's ID card: identity, owner, team, scopes, budget left and its last twenty spans. */
export const agentCardQuery = (agentId: string) =>
  queryOptions({
    queryKey: qk.agents.card(agentId),
    queryFn: ({ signal }) => apiRequest(`/agents/${agentId}`, agentCardSchema, { signal }),
    staleTime: 10_000,
  });

/** Takes the agent out of service: running nodes are cancelled, new ones refused (admins). */
export function quarantineAgent(agentId: string) {
  return apiRequest(`/agents/${agentId}/quarantine`, agentSchema, { method: 'POST' });
}

/** Puts a quarantined agent back in service (admins). */
export function releaseAgent(agentId: string) {
  return apiRequest(`/agents/${agentId}/release`, agentSchema, { method: 'POST' });
}

export function createAgent(body: AgentInput & { workspace_id?: string }) {
  return apiRequest('/agents', agentSchema, { method: 'POST', body });
}

export function updateAgent(agentId: string, body: AgentInput) {
  return apiRequest(`/agents/${agentId}`, agentSchema, { method: 'PATCH', body });
}

export function deleteAgent(agentId: string) {
  return apiSend(`/agents/${agentId}`, { method: 'DELETE' });
}

/** The API tokens of an external agent, newest first: prefixes and dates, never the tokens (admins). */
export const agentTokensQuery = (agentId: string) =>
  queryOptions({
    queryKey: qk.agents.tokens(agentId),
    queryFn: ({ signal }) =>
      apiRequest(`/agents/${agentId}/tokens`, z.array(agentApiTokenSchema), { signal }),
    staleTime: 10_000,
  });

/** Issues an API token to an external agent (admins); the secret is in the answer this once. */
export function issueAgentToken(agentId: string, name: string) {
  return apiRequest(`/agents/${agentId}/tokens`, issuedAgentApiTokenSchema, {
    method: 'POST',
    body: { name },
  });
}

/** Revokes an API token (admins); the agent's calls with it are refused at once. */
export function revokeAgentToken(agentId: string, tokenId: string) {
  return apiSend(`/agents/${agentId}/tokens/${tokenId}`, { method: 'DELETE' });
}

/** The tasks an exposed agent received over A2A, newest first (CONTRACT §10). */
export const agentA2aTasksQuery = (agentId: string, limit = 50) =>
  queryOptions({
    queryKey: qk.agents.a2aTasks(agentId),
    queryFn: ({ signal }) =>
      apiRequest(`/agents/${agentId}/a2a/tasks`, z.array(a2aTaskSchema), {
        signal,
        query: { limit },
      }),
    refetchInterval: 15_000,
  });

/** Publishes an agent over A2A, or withdraws it (admins; audited). */
export function setAgentA2aExposed(agentId: string, exposed: boolean) {
  return apiRequest(`/agents/${agentId}`, agentSchema, {
    method: 'PATCH',
    body: { a2a_exposed: exposed },
  });
}
