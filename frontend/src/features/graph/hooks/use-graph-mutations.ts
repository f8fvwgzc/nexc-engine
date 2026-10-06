import { useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';

import { ApiError, errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type {
  CreateEdgeBody,
  CreateNodeBody,
  EdgeSuggestion,
  Ontology,
  UpdateNodeBody,
} from '@/schemas/graph';
import { useGraphStore } from '@/stores/graph-store';

import {
  applyPlan,
  createEdge,
  createNode,
  deleteEdge,
  deleteNode,
  replaceOntology,
  requestPlan,
  startRun,
  updateEdge,
  updateNode,
} from '../api';
import { removeEdge, removeNode, updateGraphCache, upsertEdge, upsertNode } from '../graph-cache';

export function useCreateNode(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateNodeBody) => createNode(graphId, body),
    onSuccess: (node) => {
      updateGraphCache(queryClient, graphId, (g) => upsertNode(g, node));
      useGraphStore.getState().selectNode(node.id);
    },
  });
}

export function useUpdateNode(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ nodeId, body }: { nodeId: string; body: UpdateNodeBody }) =>
      updateNode(graphId, nodeId, body),
    onSuccess: (node) => updateGraphCache(queryClient, graphId, (g) => upsertNode(g, node)),
  });
}

export function useDeleteNode(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (nodeId: string) => deleteNode(graphId, nodeId),
    meta: { successMessage: 'Node deleted' },
    onSuccess: (_void, nodeId) => {
      updateGraphCache(queryClient, graphId, (g) => removeNode(g, nodeId));
      const store = useGraphStore.getState();
      if (store.selectedNodeId === nodeId) store.selectNode(null);
    },
  });
}

function edgeErrorToast(error: unknown) {
  if (error instanceof ApiError && error.status === 409) {
    toast.error('That dependency would create a cycle', {
      description: error.detail ?? 'Blocking relations must form a DAG.',
    });
  } else {
    toast.error(errorMessage(error));
  }
}

export function useCreateEdge(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateEdgeBody) => createEdge(graphId, body),
    meta: { errorToast: false },
    onSuccess: (edge) => updateGraphCache(queryClient, graphId, (g) => upsertEdge(g, edge)),
    onError: edgeErrorToast,
  });
}

export function useAcceptSuggestion(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (s: EdgeSuggestion) =>
      createEdge(graphId, { source: s.source, target: s.target, reason: s.reason }),
    meta: { errorToast: false, successMessage: 'Dependency added' },
    onSuccess: (edge, s) => {
      updateGraphCache(queryClient, graphId, (g) => upsertEdge(g, edge));
      queryClient.setQueryData<EdgeSuggestion[]>(qk.graphs.suggestions(graphId), (list) =>
        list?.filter((x) => x.source !== s.source || x.target !== s.target),
      );
    },
    onError: edgeErrorToast,
  });
}

export function useUpdateEdge(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ edgeId, reason }: { edgeId: string; reason: string }) =>
      updateEdge(graphId, edgeId, { reason }),
    onSuccess: (edge) => updateGraphCache(queryClient, graphId, (g) => upsertEdge(g, edge)),
  });
}

export function useReplaceOntology(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (ontology: Ontology) => replaceOntology(graphId, ontology),
    meta: { successMessage: 'Ontology saved' },
    onSuccess: (graph) => queryClient.setQueryData(qk.graphs.detail(graphId), graph),
  });
}

export function useDeleteEdge(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (edgeId: string) => deleteEdge(graphId, edgeId),
    onSuccess: (_void, edgeId) => {
      updateGraphCache(queryClient, graphId, (g) => removeEdge(g, edgeId));
      useGraphStore.getState().selectEdge(null);
    },
  });
}

export function useRequestPlan(graphId: string) {
  return useMutation({
    mutationFn: (instructions?: string) => {
      useGraphStore.getState().planRequested();
      return requestPlan(graphId, instructions);
    },
    onSuccess: (plan) => {
      const store = useGraphStore.getState();
      if (plan.status === 'streaming') store.planStarted(plan.id);
      else store.planReady(plan);
    },
    onError: () => useGraphStore.getState().clearPlan(),
  });
}

export function useApplyPlan(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (planId: string) => applyPlan(graphId, planId),
    meta: { successMessage: 'Plan applied to the graph' },
    onSuccess: (graph) => {
      queryClient.setQueryData(qk.graphs.detail(graphId), graph);
      void queryClient.invalidateQueries({ queryKey: qk.graphs.lists() });
      useGraphStore.getState().clearPlan();
    },
  });
}

export function useStartRun(graphId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (opts: { force: boolean; nodeIds?: string[] }) =>
      startRun(graphId, { force: opts.force, node_ids: opts.nodeIds }),
    onSuccess: (run) => {
      useGraphStore.getState().runUpdated(run);
      queryClient.setQueryData(qk.runs.detail(run.id), run);
      void queryClient.invalidateQueries({ queryKey: qk.graphs.runs(graphId) });
    },
  });
}
