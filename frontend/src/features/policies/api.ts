import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  alertSchema,
  approvalSchema,
  policySchema,
  resolvedPolicySchema,
  type ApprovalStatus,
  type PolicyDocument,
  type PolicyScope,
} from '@/schemas/policy';
import { artifactSchema } from '@/schemas/run';

/** Every version of the workspace's policies, newest first, or those of one level. */
export const policiesQuery = (workspaceId: string, scope?: PolicyScope, scopeId?: string) =>
  queryOptions({
    queryKey: qk.policies.list(workspaceId, scope, scopeId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/policies`, z.array(policySchema), {
        signal,
        query: { scope, scope_id: scopeId, limit: 200 },
      }),
  });

/** The effective policy of an agent and which level set each rule. */
export const resolvedPolicyQuery = (workspaceId: string, agentId: string) =>
  queryOptions({
    queryKey: qk.policies.resolved(workspaceId, agentId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/policies/resolve`, resolvedPolicySchema, {
        signal,
        query: { agent_id: agentId },
      }),
  });

/** Adds a version at one level (admins). */
export function createPolicy(
  workspaceId: string,
  body: { scope: PolicyScope; scope_id?: string; document: PolicyDocument },
) {
  return apiRequest(`/workspaces/${workspaceId}/policies`, policySchema, {
    method: 'POST',
    body,
  });
}

/** The approvals of a workspace, newest first (admins). */
export const approvalsQuery = (workspaceId: string, status?: ApprovalStatus) =>
  queryOptions({
    queryKey: qk.approvals.list(workspaceId, status),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/approvals`, z.array(approvalSchema), {
        signal,
        query: { status, limit: 100 },
      }),
    refetchInterval: 15_000,
  });

export function approveApproval(approvalId: string, reason: string) {
  return apiRequest(`/approvals/${approvalId}/approve`, approvalSchema, {
    method: 'POST',
    body: { reason },
  });
}

export function denyApproval(approvalId: string, reason: string) {
  return apiRequest(`/approvals/${approvalId}/deny`, approvalSchema, {
    method: 'POST',
    body: { reason },
  });
}

/** The alerts of a workspace, newest first (admins). */
export const alertsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: qk.alerts.list(workspaceId),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspaceId}/alerts`, z.array(alertSchema), {
        signal,
        query: { limit: 200 },
      }),
    refetchInterval: 30_000,
  });

export function acknowledgeAlert(alertId: string) {
  return apiRequest(`/alerts/${alertId}/acknowledge`, alertSchema, { method: 'POST' });
}

/** Lets a quarantined artifact be downloaded again (admins). */
export function releaseArtifact(artifactId: string) {
  return apiRequest(`/artifacts/${artifactId}/release`, artifactSchema, { method: 'POST' });
}
