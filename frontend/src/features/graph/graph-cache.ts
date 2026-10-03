import type { QueryClient } from '@tanstack/react-query';

import { qk } from '@/lib/query-keys';
import type { Graph, GraphEdge, GraphNode } from '@/schemas/graph';

/** Immutable helpers that keep the cached `Graph` in sync with REST results and WS pushes. */
export function upsertNode(graph: Graph, node: GraphNode): Graph {
  const exists = graph.nodes.some((n) => n.id === node.id);
  return {
    ...graph,
    nodes: exists ? graph.nodes.map((n) => (n.id === node.id ? node : n)) : [...graph.nodes, node],
  };
}

export function removeNode(graph: Graph, nodeId: string): Graph {
  return {
    ...graph,
    nodes: graph.nodes.filter((n) => n.id !== nodeId),
    edges: graph.edges.filter((e) => e.source !== nodeId && e.target !== nodeId),
  };
}

export function upsertEdge(graph: Graph, edge: GraphEdge): Graph {
  const exists = graph.edges.some((e) => e.id === edge.id);
  return {
    ...graph,
    edges: exists ? graph.edges.map((e) => (e.id === edge.id ? edge : e)) : [...graph.edges, edge],
  };
}

export function removeEdge(graph: Graph, edgeId: string): Graph {
  return { ...graph, edges: graph.edges.filter((e) => e.id !== edgeId) };
}

export function moveNodes(graph: Graph, positions: Map<string, { x: number; y: number }>): Graph {
  return {
    ...graph,
    nodes: graph.nodes.map((n) => {
      const p = positions.get(n.id);
      return p ? { ...n, x: p.x, y: p.y } : n;
    }),
  };
}

export function updateGraphCache(
  queryClient: QueryClient,
  graphId: string,
  update: (graph: Graph) => Graph,
): void {
  queryClient.setQueryData<Graph>(qk.graphs.detail(graphId), (graph) =>
    graph ? update(graph) : graph,
  );
}
