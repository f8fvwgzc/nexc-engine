import { pointer } from 'd3';
import { useImperativeHandle, useMemo, type MouseEvent, type PointerEvent, type Ref } from 'react';

import type { EdgeSuggestion, Graph } from '@/schemas/graph';
import type { Cursor } from '@/schemas/realtime';
import { useGraphStore } from '@/stores/graph-store';

import type { CanvasApi } from './canvas-api';
import { CanvasDefs } from './canvas-defs';
import { EdgeLayer, SuggestionLayer } from './edge-layer';
import type { Point } from './geometry';
import { GhostLayer } from './ghost-layer';
import { layoutGhosts } from './layout';
import { NodeLayer } from './node-layer';
import { PresenceLayer } from './presence-layer';
import { useD3Canvas } from './use-d3-canvas';

export interface GraphCanvasProps {
  graph: Graph;
  suggestions: EdgeSuggestion[];
  apiRef: Ref<CanvasApi>;
  onCreateNodeAt: (point: Point) => void;
  onMoveNode: (nodeId: string, x: number, y: number, final: boolean) => void;
  onConnect: (sourceId: string, targetId: string) => void;
  onAcceptSuggestion: (suggestion: EdgeSuggestion) => void;
  onCursorMove: (cursor: Cursor) => void;
}

const INTERACTIVE = '[data-node-id],[data-edge-id],[data-suggestion]';

function isBackground(event: MouseEvent): boolean {
  return !(event.target as Element).closest(INTERACTIVE);
}

/** Obsidian-style graph canvas. Default export so it can be code-split with React.lazy. */
export default function GraphCanvas({
  graph,
  suggestions,
  apiRef,
  onCreateNodeAt,
  onMoveNode,
  onConnect,
  onAcceptSuggestion,
  onCursorMove,
}: GraphCanvasProps) {
  const liveStates = useGraphStore((s) => s.nodeStates);
  const selectedNodeId = useGraphStore((s) => s.selectedNodeId);
  const selectedEdgeId = useGraphStore((s) => s.selectedEdgeId);
  const selectNode = useGraphStore((s) => s.selectNode);
  const selectEdge = useGraphStore((s) => s.selectEdge);
  const plan = useGraphStore((s) => s.plan);
  const peers = useGraphStore((s) => s.peers);
  const setTransform = useGraphStore((s) => s.setTransform);
  const initialTransform = useGraphStore((s) => s.transforms[graph.id]);

  const ghostPositions = useMemo(() => {
    if (!plan || plan.nodes.length === 0) return new Map<string, Point>();
    const anchors = new Map(graph.nodes.map((n) => [n.id, { x: n.x, y: n.y }]));
    return layoutGhosts(plan.nodes, anchors);
  }, [plan, graph.nodes]);

  const visibleSuggestions = useMemo(() => {
    const ids = new Set(graph.nodes.map((n) => n.id));
    const existing = new Set(graph.edges.map((e) => `${e.source}->${e.target}`));
    return suggestions.filter(
      (s) => ids.has(s.source) && ids.has(s.target) && !existing.has(`${s.source}->${s.target}`),
    );
  }, [suggestions, graph.nodes, graph.edges]);

  const { svgRef, viewportRef, previewRef, api } = useD3Canvas({
    graphId: graph.id,
    nodes: graph.nodes,
    edges: graph.edges,
    ghostPositions,
    initialTransform,
    onMoveNode,
    onConnect,
    onTransformEnd: (t) => setTransform(graph.id, t),
  });
  useImperativeHandle(apiRef, () => api, [api]);

  const toGraphPoint = (event: MouseEvent | PointerEvent): Point | null => {
    const viewport = viewportRef.current;
    if (!viewport) return null;
    const [x, y] = pointer(event.nativeEvent, viewport);
    return { x, y };
  };

  return (
    <svg
      ref={svgRef}
      className="graph-canvas block size-full outline-none"
      role="application"
      aria-roledescription="graph canvas"
      aria-label={`${graph.name}: ${graph.nodes.length} nodes, ${graph.edges.length} edges. Double-click empty space to add a node.`}
      onClick={(e) => {
        if (isBackground(e)) {
          selectNode(null);
          selectEdge(null);
        }
      }}
      onDoubleClick={(e) => {
        if (!isBackground(e)) return;
        const point = toGraphPoint(e);
        if (point) onCreateNodeAt(point);
      }}
      onPointerMove={(e) => onCursorMove(toGraphPoint(e))}
      onPointerLeave={() => onCursorMove(null)}
    >
      <CanvasDefs />
      <g ref={viewportRef}>
        <EdgeLayer edges={graph.edges} selectedEdgeId={selectedEdgeId} onSelect={selectEdge} />
        <SuggestionLayer suggestions={visibleSuggestions} onAccept={onAcceptSuggestion} />
        <NodeLayer
          nodes={graph.nodes}
          liveStates={liveStates}
          selectedNodeId={selectedNodeId}
          onSelect={selectNode}
        />
        {plan && <GhostLayer plan={plan} positions={ghostPositions} />}
        <PresenceLayer peers={peers} />
        <path ref={previewRef} className="glink-preview" visibility="hidden" />
      </g>
    </svg>
  );
}
