import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';

import { qk } from '@/lib/query-keys';
import type { ConnectionState } from '@/lib/realtime/connection-state';
import { GraphSocket } from '@/lib/realtime/ws';
import type { GraphSummary } from '@/schemas/graph';
import type { Cursor, WsServerMessage } from '@/schemas/realtime';
import { useAuthStore } from '@/stores/auth-store';
import { useGraphStore } from '@/stores/graph-store';

import { removeEdge, removeNode, updateGraphCache, upsertEdge, upsertNode } from '../graph-cache';

const PEER_TTL_MS = 30_000;

export interface GraphSocketApi {
  state: ConnectionState;
  isOpen: () => boolean;
  moveNode: (nodeId: string, x: number, y: number) => void;
  flushMoves: () => void;
  sendPresence: (cursor: Cursor) => void;
}

/** Collaborative channel: node/edge broadcasts, dependency suggestions, presence, node moves. */
export function useGraphSocket(graphId: string): GraphSocketApi {
  const queryClient = useQueryClient();
  const socketRef = useRef<GraphSocket | null>(null);
  const [state, setState] = useState<ConnectionState>('connecting');

  useEffect(() => {
    const onMessage = (message: WsServerMessage) => {
      const store = useGraphStore.getState();
      switch (message.type) {
        case 'node.upserted':
          updateGraphCache(queryClient, graphId, (g) => upsertNode(g, message.node));
          break;
        case 'node.deleted':
          updateGraphCache(queryClient, graphId, (g) => removeNode(g, message.node_id));
          if (store.selectedNodeId === message.node_id) store.selectNode(null);
          break;
        case 'edge.upserted':
          updateGraphCache(queryClient, graphId, (g) => upsertEdge(g, message.edge));
          break;
        case 'edge.deleted':
          updateGraphCache(queryClient, graphId, (g) => removeEdge(g, message.edge_id));
          if (store.selectedEdgeId === message.edge_id) store.selectEdge(null);
          break;
        case 'graph.updated': {
          const { name, description } = message.graph;
          updateGraphCache(queryClient, graphId, (g) => ({ ...g, name, description }));
          queryClient.setQueriesData<GraphSummary[]>({ queryKey: qk.graphs.lists() }, (list) =>
            list?.map((s) => (s.id === message.graph.id ? message.graph : s)),
          );
          break;
        }
        case 'ontology.updated':
          updateGraphCache(queryClient, graphId, (g) => ({ ...g, ontology: message.ontology }));
          break;
        case 'suggestions':
          queryClient.setQueryData(qk.graphs.suggestions(graphId), message.items);
          break;
        case 'presence':
          if (message.user_id !== useAuthStore.getState().user?.id) {
            store.upsertPeer(message.user_id, message.name, message.cursor);
          }
          break;
        case 'pong':
          break;
      }
    };

    const socket = new GraphSocket(graphId, {
      onMessage,
      onStateChange: setState,
      onReconnect: () =>
        void queryClient.invalidateQueries({ queryKey: qk.graphs.detail(graphId), exact: true }),
    });
    socketRef.current = socket;
    socket.start();
    const prune = setInterval(() => useGraphStore.getState().prunePeers(PEER_TTL_MS), 10_000);
    return () => {
      clearInterval(prune);
      socket.stop();
      socketRef.current = null;
    };
  }, [graphId, queryClient]);

  return useMemo<GraphSocketApi>(
    () => ({
      state,
      isOpen: () => socketRef.current?.isOpen ?? false,
      moveNode: (nodeId, x, y) => socketRef.current?.moveNode(nodeId, x, y),
      flushMoves: () => socketRef.current?.flushMoves(),
      sendPresence: (cursor) => socketRef.current?.sendPresence(cursor),
    }),
    [state],
  );
}
