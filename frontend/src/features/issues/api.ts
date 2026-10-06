import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  issueSchema,
  issueStateSchema,
  projectSchema,
  type IssueInput,
  type ProjectStatus,
} from '@/schemas/issue';

export interface IssueFilter {
  team_id?: string;
  project_id?: string;
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
  body: { status?: ProjectStatus; name?: string },
) {
  return apiRequest(`/workspaces/${workspaceId}/projects/${projectId}`, projectSchema, {
    method: 'PATCH',
    body,
  });
}

export function deleteProject(workspaceId: string, projectId: string) {
  return apiSend(`/workspaces/${workspaceId}/projects/${projectId}`, { method: 'DELETE' });
}
