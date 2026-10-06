import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import { cycleSchema } from '@/schemas/cycle';
import {
  issueEventSchema,
  issueSchema,
  issueStateSchema,
  labelSchema,
  notificationSchema,
  projectSchema,
  type IssueInput,
  type ProjectStatus,
  type StateCategory,
} from '@/schemas/issue';

export interface IssueFilter {
  team_id?: string;
  project_id?: string;
  label_id?: string;
  parent_id?: string;
  cycle_id?: string;
  /** Only issues assigned to this person. */
  assignee_id?: string;
  /** Only issues this person filed. */
  creator_id?: string;
  open?: boolean;
  q?: string;
}

/** Issues of a workspace in the teams the caller can see. */
export const issuesQuery = (workspaceId: string, filter: IssueFilter = {}) =>
  queryOptions({
    queryKey: qk.issues.list(workspaceId, filter),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/issues`, z.array(issueSchema), {
        query: {
          team_id: filter.team_id,
          project_id: filter.project_id,
          label_id: filter.label_id,
          parent_id: filter.parent_id,
          cycle_id: filter.cycle_id,
          assignee_id: filter.assignee_id,
          creator_id: filter.creator_id,
          open: filter.open ? 'true' : undefined,
          q: filter.q,
        },
        signal,
      }),
  });

export const statesQuery = (workspaceId: string, teamId: string) =>
  queryOptions({
    queryKey: qk.issues.states(workspaceId, teamId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/states`, z.array(issueStateSchema), {
        signal,
      }),
    staleTime: 5 * 60_000,
  });

export interface StateInput {
  name: string;
  category: StateCategory;
  color: string;
  position?: number;
}

export function createState(workspaceId: string, teamId: string, body: StateInput) {
  return apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/states`, issueStateSchema, {
    method: 'POST',
    body,
  });
}

export function updateState(
  workspaceId: string,
  teamId: string,
  stateId: string,
  body: Partial<StateInput>,
) {
  return apiRequest(
    `/workspaces/${workspaceId}/teams/${teamId}/states/${stateId}`,
    issueStateSchema,
    { method: 'PATCH', body },
  );
}

export function deleteState(workspaceId: string, teamId: string, stateId: string) {
  return apiSend(`/workspaces/${workspaceId}/teams/${teamId}/states/${stateId}`, {
    method: 'DELETE',
  });
}

export function createIssue(workspaceId: string, teamId: string, body: IssueInput) {
  return apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/issues`, issueSchema, {
    method: 'POST',
    body,
  });
}

export function updateIssue(issueId: string, body: Partial<IssueInput>) {
  return apiRequest(`/issues/${issueId}`, issueSchema, { method: 'PATCH', body });
}

export function deleteIssue(issueId: string) {
  return apiSend(`/issues/${issueId}`, { method: 'DELETE' });
}

/** Gives the issue a graph that plans and executes it (or returns the one it has). */
export function createIssueGraph(issueId: string) {
  return apiRequest(`/issues/${issueId}/graph`, issueSchema, { method: 'POST' });
}

/** One issue, for opening it from a link when it is not in the list on screen. */
export const issueQuery = (issueId: string) =>
  queryOptions({
    queryKey: qk.issues.one(issueId),
    queryFn: ({ signal }) => apiRequest(`/issues/${issueId}`, issueSchema, { signal }),
  });

/** The caller's inbox in a workspace, newest first; checked again every minute. */
export const inboxQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.issues.inbox(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/inbox`, z.array(notificationSchema), { signal }),
    refetchInterval: 60_000,
  });

/** Marks the given notifications read, or all of them. */
export function markInboxRead(workspaceId: string, ids?: string[]) {
  return apiRequest(`/workspaces/${workspaceId}/inbox/read`, z.array(notificationSchema), {
    method: 'POST',
    body: ids ? { ids } : {},
  });
}

/** The cycles of a team, latest first. */
export const cyclesQuery = (workspaceId: string, teamId: string) =>
  queryOptions({
    queryKey: qk.issues.cycles(workspaceId, teamId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/cycles`, z.array(cycleSchema), {
        signal,
      }),
    staleTime: 60_000,
  });

export function createCycle(
  workspaceId: string,
  teamId: string,
  body: { name: string; starts_on: string; ends_on: string },
) {
  return apiRequest(`/workspaces/${workspaceId}/teams/${teamId}/cycles`, cycleSchema, {
    method: 'POST',
    body,
  });
}

export function deleteCycle(workspaceId: string, teamId: string, cycleId: string) {
  return apiSend(`/workspaces/${workspaceId}/teams/${teamId}/cycles/${cycleId}`, {
    method: 'DELETE',
  });
}

/** The labels of a workspace, by name. */
export const labelsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.issues.labels(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/labels`, z.array(labelSchema), { signal }),
    staleTime: 5 * 60_000,
  });

export function createLabel(workspaceId: string, body: { name: string; color: string }) {
  return apiRequest(`/workspaces/${workspaceId}/labels`, labelSchema, { method: 'POST', body });
}

export function deleteLabel(workspaceId: string, labelId: string) {
  return apiSend(`/workspaces/${workspaceId}/labels/${labelId}`, { method: 'DELETE' });
}

/** The timeline of an issue, oldest first: comments and changes. */
export const issueEventsQuery = (issueId: string) =>
  queryOptions({
    queryKey: qk.issues.events(issueId),
    queryFn: ({ signal }) =>
      apiRequest(`/issues/${issueId}/events`, z.array(issueEventSchema), { signal }),
  });

export function createComment(issueId: string, body: string) {
  return apiRequest(`/issues/${issueId}/comments`, issueEventSchema, {
    method: 'POST',
    body: { body },
  });
}

export function updateComment(issueId: string, commentId: string, body: string) {
  return apiRequest(`/issues/${issueId}/comments/${commentId}`, issueEventSchema, {
    method: 'PATCH',
    body: { body },
  });
}

export function deleteComment(issueId: string, commentId: string) {
  return apiSend(`/issues/${issueId}/comments/${commentId}`, { method: 'DELETE' });
}

export const projectsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.issues.projects(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/projects`, z.array(projectSchema), { signal }),
  });

export function createProject(
  workspaceId: string,
  body: { name: string; description?: string; target_date?: string },
) {
  return apiRequest(`/workspaces/${workspaceId}/projects`, projectSchema, { method: 'POST', body });
}

export function updateProject(
  workspaceId: string,
  projectId: string,
  body: {
    status?: ProjectStatus;
    name?: string;
    description?: string;
    lead_id?: string | null;
    target_date?: string | null;
  },
) {
  return apiRequest(`/workspaces/${workspaceId}/projects/${projectId}`, projectSchema, {
    method: 'PATCH',
    body,
  });
}

export function deleteProject(workspaceId: string, projectId: string) {
  return apiSend(`/workspaces/${workspaceId}/projects/${projectId}`, { method: 'DELETE' });
}
