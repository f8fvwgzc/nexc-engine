import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { assistantReplySchema, type AssistantTurn } from '@/schemas/assistant';
import { usageReportSchema } from '@/schemas/usage';
import {
  guardrailsSchema,
  inviteResultSchema,
  teamMemberSchema,
  teamSchema,
  workspaceInviteSchema,
  workspaceMemberSchema,
  workspaceSchema,
  type Guardrails,
  type TeamRole,
  type WorkspaceRole,
} from '@/schemas/workspace';

export const workspacesQuery = () =>
  queryOptions({
    queryKey: qk.workspaces.all,
    queryFn: ({ signal }) => apiRequest('/workspaces', z.array(workspaceSchema), { signal }),
    staleTime: 60_000,
  });

export function createWorkspace(body: { name: string }) {
  return apiRequest('/workspaces', workspaceSchema, { method: 'POST', body });
}

export const membersQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.workspaces.members(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/members`, z.array(workspaceMemberSchema), { signal }),
  });

export const invitesQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.workspaces.invites(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/invites`, z.array(workspaceInviteSchema), { signal }),
  });

export function inviteMember(workspaceId: string, body: { email: string; role: WorkspaceRole }) {
  return apiRequest(`/workspaces/${workspaceId}/members`, inviteResultSchema, {
    method: 'POST',
    body,
  });
}

export function setMemberRole(workspaceId: string, userId: string, role: WorkspaceRole) {
  return apiRequest(
    `/workspaces/${workspaceId}/members/${userId}`,
    z.array(workspaceMemberSchema),
    { method: 'PATCH', body: { role } },
  );
}

export function removeMember(workspaceId: string, userId: string) {
  return apiSend(`/workspaces/${workspaceId}/members/${userId}`, { method: 'DELETE' });
}

export function withdrawInvite(workspaceId: string, inviteId: string) {
  return apiSend(`/workspaces/${workspaceId}/invites/${inviteId}`, { method: 'DELETE' });
}

/** Token usage of a workspace over the last `days` days. */
export const usageQuery = (workspaceId: string, days: number) =>
  queryOptions({
    queryKey: qk.workspaces.usage(workspaceId, days),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/usage?days=${days}`, usageReportSchema, { signal }),
  });

export const guardrailsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.workspaces.guardrails(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/guardrails`, guardrailsSchema, { signal }),
  });

export function saveGuardrails(workspaceId: string, body: Guardrails) {
  return apiRequest(`/workspaces/${workspaceId}/guardrails`, guardrailsSchema, {
    method: 'PUT',
    body,
  });
}

/** Asks the workspace assistant; it may file issues on the caller's behalf. */
export function askAssistant(workspaceId: string, message: string, history: AssistantTurn[]) {
  return apiRequest(`/workspaces/${workspaceId}/assistant`, assistantReplySchema, {
    method: 'POST',
    body: { message, history },
  });
}

export const teamsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.workspaces.teams(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/teams`, z.array(teamSchema), { signal }),
  });

export const teamMembersQuery = (workspaceId: string, teamId: string) =>
  queryOptions({
    queryKey: qk.workspaces.teamMembers(workspaceId, teamId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/members`, z.array(teamMemberSchema), {
        signal,
      }),
  });

export interface CreateTeamBody {
  name: string;
  key?: string;
  description?: string;
  private?: boolean;
}

export function createTeam(workspaceId: string, body: CreateTeamBody) {
  return apiRequest(`/workspaces/${workspaceId}/teams`, teamSchema, { method: 'POST', body });
}

export function deleteTeam(workspaceId: string, teamId: string) {
  return apiSend(`/workspaces/${workspaceId}/teams/${teamId}`, { method: 'DELETE' });
}

export function setTeamMember(
  workspaceId: string,
  teamId: string,
  userId: string,
  role?: TeamRole,
) {
  return apiRequest(
    `/workspaces/${workspaceId}/teams/${teamId}/members/${userId}`,
    z.array(teamMemberSchema),
    { method: 'PUT', body: role ? { role } : {} },
  );
}

export function removeTeamMember(workspaceId: string, teamId: string, userId: string) {
  return apiSend(`/workspaces/${workspaceId}/teams/${teamId}/members/${userId}`, {
    method: 'DELETE',
  });
}
