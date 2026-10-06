import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';

import { useWorkspaceId } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import type { Graph, GraphInput, GraphSummary } from '@/schemas/graph';
import type { CreateFromTemplateBody } from '@/schemas/template';

import { createGraph, createGraphFromTemplate, deleteGraph } from '../api';

/** Caches the new graph and opens its canvas. */
function useOpenCreatedGraph() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  return (graph: Graph) => {
    queryClient.setQueryData(qk.graphs.detail(graph.id), graph);
    void queryClient.invalidateQueries({ queryKey: qk.graphs.lists() });
    void navigate(`/app/graphs/${graph.id}`);
  };
}

export function useCreateGraph() {
  const open = useOpenCreatedGraph();
  const workspaceId = useWorkspaceId();
  return useMutation({
    mutationFn: (body: GraphInput) => createGraph({ ...body, workspace_id: workspaceId }),
    meta: { errorToast: false },
    onSuccess: open,
  });
}

export function useCreateFromTemplate() {
  const open = useOpenCreatedGraph();
  const workspaceId = useWorkspaceId();
  return useMutation({
    mutationFn: (body: CreateFromTemplateBody) =>
      createGraphFromTemplate({ ...body, workspace_id: workspaceId }),
    meta: { successMessage: 'Graph created from template' },
    onSuccess: open,
  });
}

export function useDeleteGraph() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteGraph,
    meta: { successMessage: 'Graph deleted' },
    onSuccess: (_void, graphId) => {
      queryClient.setQueriesData<GraphSummary[]>({ queryKey: qk.graphs.lists() }, (list) =>
        list?.filter((g) => g.id !== graphId),
      );
      queryClient.removeQueries({ queryKey: qk.graphs.detail(graphId) });
    },
  });
}
