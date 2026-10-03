import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';
import { toast } from 'sonner';

import type { Point } from '../canvas/geometry';
import { layoutLevels } from '../canvas/layout';
import { fetchAnalysis, updateNode } from '../api';
import { moveNodes, updateGraphCache } from '../graph-cache';
import type { GraphSocketApi } from './use-graph-socket';

/**
 * Persists node positions: streamed over the WebSocket (`node.move`, throttled) while connected,
 * falling back to PATCH when the socket is down.
 */
export function useNodePositions(graphId: string, socket: GraphSocketApi) {
  const queryClient = useQueryClient();

  const persist = useCallback(
    (positions: Map<string, Point>) => {
      updateGraphCache(queryClient, graphId, (g) => moveNodes(g, positions));
      if (socket.isOpen()) {
        for (const [id, p] of positions) socket.moveNode(id, p.x, p.y);
        socket.flushMoves();
        return;
      }
      for (const [id, p] of positions) {
        updateNode(graphId, id, { x: Math.round(p.x), y: Math.round(p.y) }).catch(() =>
          toast.error('Could not save the node position'),
        );
      }
    },
    [graphId, queryClient, socket],
  );

  /** Live drag updates (WS only — cheap, throttled) and the final drop (persisted + cached). */
  const moveNode = useCallback(
    (nodeId: string, x: number, y: number, final: boolean) => {
      if (final) persist(new Map([[nodeId, { x, y }]]));
      else socket.moveNode(nodeId, x, y);
    },
    [persist, socket],
  );

  const autoLayout = useMutation({
    mutationFn: () => fetchAnalysis(graphId),
    onSuccess: (analysis) => {
      if (analysis.cycles.length > 0) {
        toast.warning('The graph has dependency cycles; layout may look odd.');
      }
      persist(layoutLevels(analysis.levels));
    },
  });

  return { moveNode, autoLayout };
}
