import { useQuery, useSuspenseQuery } from '@tanstack/react-query';
import { lazy, Suspense, useCallback, useLayoutEffect, useRef, useState } from 'react';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { OntologyContext } from '@/components/custom-ui/node-kind-meta';
import { graphQuery } from '@/features/graphs/api';
import type { EdgeSuggestion } from '@/schemas/graph';
import { useGraphStore } from '@/stores/graph-store';

import { suggestionsQuery } from '../api';
import type { CanvasApi } from '../canvas/canvas-api';
import type { Point } from '../canvas/geometry';
import { useCanvasShortcuts } from '../hooks/use-canvas-shortcuts';
import { useGraphCommands } from '../hooks/use-graph-commands';
import { useGraphEvents } from '../hooks/use-graph-events';
import {
  useAcceptSuggestion,
  useCreateEdge,
  useCreateNode,
  useDeleteEdge,
  useDeleteNode,
  useRequestPlan,
  useStartRun,
} from '../hooks/use-graph-mutations';
import { useGraphSocket } from '../hooks/use-graph-socket';
import { useHydrateLatestRun } from '../hooks/use-hydrate-latest-run';
import { useNodePositions } from '../hooks/use-node-positions';
import { EdgeInspector } from '../panels/edge-inspector';
import { NodePanel } from '../panels/node-panel';
import { GraphToolbar } from '../toolbar/graph-toolbar';
import { RunProgress } from '../toolbar/run-progress';
import { CanvasEmptyHint } from './canvas-empty-hint';
import { CanvasLegend } from './canvas-legend';
import { CanvasSkeleton } from './canvas-skeleton';
import { GraphSeo } from './graph-seo';

// d3 + the canvas are split into their own chunk.
const GraphCanvas = lazy(() => import('../canvas/graph-canvas'));

type DeleteTarget = { kind: 'node' | 'edge'; id: string };

const NO_SUGGESTIONS: EdgeSuggestion[] = [];

export function GraphWorkspace({ graphId }: { graphId: string }) {
  const { data: graph } = useSuspenseQuery(graphQuery(graphId));
  const { data: suggestions = [] } = useQuery(suggestionsQuery(graphId));
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId);
  const canvasRef = useRef<CanvasApi>(null);
  const [pendingDelete, setPendingDelete] = useState<DeleteTarget | null>(null);
  // Suggestions are opt-in: on a planned graph they would bury the real edges.
  const [showSuggestions, setShowSuggestions] = useState(false);

  useLayoutEffect(() => useGraphStore.getState().enterGraph(graphId), [graphId]);
  useHydrateLatestRun(graphId);
  const sse = useGraphEvents(graphId);
  const socket = useGraphSocket(graphId);
  const { moveNode, autoLayout } = useNodePositions(graphId, socket);

  // `mutate` functions are referentially stable, so the callbacks below are too.
  const { mutate: createNode } = useCreateNode(graphId);
  const { mutate: createEdge } = useCreateEdge(graphId);
  const { mutate: acceptSuggestion } = useAcceptSuggestion(graphId);
  const { mutate: deleteNode } = useDeleteNode(graphId);
  const { mutate: deleteEdge } = useDeleteEdge(graphId);
  const { mutate: requestPlan } = useRequestPlan(graphId);
  const { mutate: startRun } = useStartRun(graphId);
  const { mutate: runAutoLayout, isPending: layoutPending } = autoLayout;

  const addNodeAt = useCallback(
    ({ x, y }: Point) => createNode({ title: 'Untitled', x: Math.round(x), y: Math.round(y) }),
    [createNode],
  );
  const addNode = useCallback(
    () => addNodeAt(canvasRef.current?.viewportCenter() ?? { x: 0, y: 0 }),
    [addNodeAt],
  );
  const fit = useCallback(() => canvasRef.current?.fitToView(), []);
  const layout = useCallback(() => {
    runAutoLayout(undefined, { onSuccess: () => setTimeout(fit, 450) });
  }, [runAutoLayout, fit]);
  const plan = useCallback(() => requestPlan(undefined), [requestPlan]);
  const run = useCallback(() => startRun({ force: false }), [startRun]);

  useCanvasShortcuts({ addNode, fit, autoLayout: layout, requestDelete: setPendingDelete });
  useGraphCommands({
    addNode,
    autoLayout: layout,
    fit,
    requestPlan: plan,
    run,
    nodeCount: graph.nodes.length,
  });

  const confirmDelete = () => {
    if (!pendingDelete) return;
    if (pendingDelete.kind === 'node') deleteNode(pendingDelete.id);
    else deleteEdge(pendingDelete.id);
    setPendingDelete(null);
  };
  const nodeToDelete =
    pendingDelete?.kind === 'node' ? graph.nodes.find((n) => n.id === pendingDelete.id) : undefined;

  return (
    <OntologyContext value={graph.ontology}>
      <div className="flex min-h-0 flex-1">
        <GraphSeo name={graph.name} description={graph.description} />
        <div className="relative min-h-[60svh] min-w-0 flex-1">
          <Suspense fallback={<CanvasSkeleton />}>
            <GraphCanvas
              key={graph.id}
              graph={graph}
              suggestions={showSuggestions ? suggestions : NO_SUGGESTIONS}
              apiRef={canvasRef}
              onCreateNodeAt={addNodeAt}
              onMoveNode={moveNode}
              onConnect={(source, target) => createEdge({ source, target })}
              onAcceptSuggestion={acceptSuggestion}
              onCursorMove={socket.sendPresence}
            />
          </Suspense>
          {graph.nodes.length === 0 && <CanvasEmptyHint />}
          {graph.nodes.length > 0 && (
            <div className="pointer-events-none absolute bottom-3 left-3 hidden md:block">
              <CanvasLegend
                suggestionCount={suggestions.length}
                showSuggestions={showSuggestions}
                onShowSuggestions={setShowSuggestions}
              />
            </div>
          )}
          <div className="pointer-events-none absolute inset-x-3 top-3 flex justify-center">
            <GraphToolbar
              graph={graph}
              nodeCount={graph.nodes.length}
              sse={sse}
              ws={socket.state}
              layoutPending={layoutPending}
              onAddNode={addNode}
              onAutoLayout={layout}
              onFit={fit}
              onDeleteEdge={(id) => setPendingDelete({ kind: 'edge', id })}
              onPlanApplied={layout}
            />
          </div>
          <div className="pointer-events-none absolute inset-x-3 bottom-3 flex flex-col items-center gap-2">
            <EdgeInspector
              graph={graph}
              onDelete={(id) => setPendingDelete({ kind: 'edge', id })}
            />
            <RunProgress />
          </div>
        </div>
        <NodePanel
          node={graph.nodes.find((n) => n.id === selectedNodeId)}
          onDelete={(id) => setPendingDelete({ kind: 'node', id })}
        />
        <ConfirmDialog
          open={pendingDelete !== null}
          onOpenChange={(open) => !open && setPendingDelete(null)}
          title={pendingDelete?.kind === 'edge' ? 'Delete this edge?' : 'Delete this node?'}
          description={
            pendingDelete?.kind === 'edge'
              ? 'The relation is removed; both nodes stay.'
              : `“${nodeToDelete?.title ?? 'Node'}” and all of its edges will be removed. This cannot be undone.`
          }
          confirmLabel="Delete"
          destructive
          onConfirm={confirmDelete}
        />
      </div>
    </OntologyContext>
  );
}
