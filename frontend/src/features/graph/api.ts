import { queryOptions } from '@tanstack/react-query';
import { z } from 'zod';

import { apiRequest, apiSend } from '@/lib/api/client';
import { qk } from '@/lib/query-keys';
import {
  edgeSuggestionSchema,
  graphAnalysisSchema,
  graphEdgeSchema,
  graphNodeSchema,
  graphSchema,
  type CreateEdgeBody,
  type Ontology,
  type CreateNodeBody,
  type UpdateNodeBody,
} from '@/schemas/graph';
import { planSchema } from '@/schemas/plan';
import { runSchema, type StartRunBody } from '@/schemas/run';

export const suggestionsQuery = (graphId: string) =>
  queryOptions({
    queryKey: qk.graphs.suggestions(graphId),
    queryFn: ({ signal }) =>
      apiRequest(`/graphs/${graphId}/suggestions`, z.array(edgeSuggestionSchema), { signal }),
  });

export function fetchAnalysis(graphId: string) {
  return apiRequest(`/graphs/${graphId}/analysis`, graphAnalysisSchema);
}

export function createNode(graphId: string, body: CreateNodeBody) {
  return apiRequest(`/graphs/${graphId}/nodes`, graphNodeSchema, { method: 'POST', body });
}

export function updateNode(graphId: string, nodeId: string, body: UpdateNodeBody) {
  return apiRequest(`/graphs/${graphId}/nodes/${nodeId}`, graphNodeSchema, {
    method: 'PATCH',
    body,
  });
}

export function deleteNode(graphId: string, nodeId: string) {
  return apiSend(`/graphs/${graphId}/nodes/${nodeId}`, { method: 'DELETE' });
}

export function createEdge(graphId: string, body: CreateEdgeBody) {
  return apiRequest(`/graphs/${graphId}/edges`, graphEdgeSchema, { method: 'POST', body });
}

export function updateEdge(graphId: string, edgeId: string, body: { reason: string }) {
  return apiRequest(`/graphs/${graphId}/edges/${edgeId}`, graphEdgeSchema, {
    method: 'PATCH',
    body,
  });
}

/** Replaces the graph's node types and relation types. */
export function replaceOntology(graphId: string, ontology: Ontology) {
  return apiRequest(`/graphs/${graphId}/ontology`, graphSchema, { method: 'PUT', body: ontology });
}

export function deleteEdge(graphId: string, edgeId: string) {
  return apiSend(`/graphs/${graphId}/edges/${edgeId}`, { method: 'DELETE' });
}

export function requestPlan(graphId: string, instructions?: string) {
  return apiRequest(`/graphs/${graphId}/plan`, planSchema, {
    method: 'POST',
    body: instructions ? { instructions } : {},
  });
}

export function fetchPlan(graphId: string, planId: string) {
  return apiRequest(`/graphs/${graphId}/plans/${planId}`, planSchema);
}

export function applyPlan(graphId: string, planId: string) {
  return apiRequest(`/graphs/${graphId}/plans/${planId}/apply`, graphSchema, { method: 'POST' });
}

export function startRun(graphId: string, body: StartRunBody) {
  return apiRequest(`/graphs/${graphId}/runs`, runSchema, { method: 'POST', body });
}
