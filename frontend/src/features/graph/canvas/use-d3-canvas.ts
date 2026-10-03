import { useLayoutEffect, useMemo, useRef } from 'react';

import type { GraphEdge, GraphNode } from '@/schemas/graph';
import type { ViewTransform } from '@/stores/graph-store';

import { CanvasEngine, type EngineCallbacks } from './canvas-engine';
import type { Point } from './geometry';

export interface D3CanvasOptions extends EngineCallbacks {
  graphId: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** Positions of streamed plan proposals (by ref), so ghost edges can attach to them. */
  ghostPositions: Map<string, Point>;
  initialTransform: ViewTransform | undefined;
}

/** Binds a CanvasEngine (D3 simulation/zoom/drag) to the React-rendered <svg>. */
export function useD3Canvas({
  graphId,
  nodes,
  edges,
  ghostPositions,
  initialTransform,
  onMoveNode,
  onConnect,
  onTransformEnd,
}: D3CanvasOptions) {
  const svgRef = useRef<SVGSVGElement>(null);
  const viewportRef = useRef<SVGGElement>(null);
  const previewRef = useRef<SVGPathElement>(null);
  const engine = useMemo(() => new CanvasEngine(), []);
  const initial = useRef(initialTransform);

  // Layout effects run in declaration order: mount → data sync → DOM indexing.
  useLayoutEffect(() => {
    engine.setCallbacks({ onMoveNode, onConnect, onTransformEnd });
  });

  useLayoutEffect(() => {
    const svg = svgRef.current;
    const viewport = viewportRef.current;
    const preview = previewRef.current;
    if (!svg || !viewport || !preview) return;
    return engine.mount(svg, viewport, preview, initial.current);
  }, [engine, graphId]);

  useLayoutEffect(() => engine.syncData(nodes, edges), [engine, nodes, edges]);

  // Every render may add/remove SVG elements (nodes, edges, suggestions, ghosts).
  useLayoutEffect(() => engine.indexElements(ghostPositions));

  return { svgRef, viewportRef, previewRef, api: engine };
}
