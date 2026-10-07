import {
  drag,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  select,
  selectAll,
  zoom,
  zoomIdentity,
  zoomTransform,
  type ForceLink,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
  type ZoomBehavior,
} from 'd3';

import { prefersReducedMotion } from '@/hooks/use-reduced-motion';
import type { GraphEdge, GraphNode } from '@/schemas/graph';
import type { ViewTransform } from '@/stores/graph-store';

import type { CanvasApi } from './canvas-api';
import { edgePath, NODE_HEIGHT, NODE_WIDTH, previewPath, type Point } from './geometry';
import { boundsOf } from './layout';
import { animateZoom } from './zoom-animation';

interface SimNode extends SimulationNodeDatum {
  id: string;
  /** Persisted position; a soft force pulls the node back toward it (Obsidian-like float). */
  anchorX: number;
  anchorY: number;
}

type SimLink = SimulationLinkDatum<SimNode>;

interface LinkElement {
  source: string;
  target: string;
  paths: SVGPathElement[];
}

export interface EngineCallbacks {
  onMoveNode: (nodeId: string, x: number, y: number, final: boolean) => void;
  onConnect: (sourceId: string, targetId: string) => void;
  onTransformEnd: (transform: ViewTransform) => void;
}

interface DragEvent {
  x: number;
  y: number;
  sourceEvent?: MouseEvent | TouchEvent;
}

const ANCHOR_STRENGTH = 0.14;

function nodeIdAt(event: MouseEvent | TouchEvent | undefined): string | null {
  if (!event) return null;
  const point = 'changedTouches' in event ? event.changedTouches[0] : event;
  if (!point) return null;
  const el = document.elementFromPoint(point.clientX, point.clientY);
  return el?.closest<SVGGElement>('[data-node-id]')?.dataset.nodeId ?? null;
}

/**
 * Imperative D3 side of the canvas. React renders every SVG element (keyed, memoized); the engine
 * owns the force simulation, zoom and drag, and writes geometry (`transform`, `d`) straight to the
 * DOM on each tick — no React state update per frame.
 */
export class CanvasEngine implements CanvasApi {
  private callbacks: EngineCallbacks | null = null;
  private svg: SVGSVGElement | null = null;
  private viewport: SVGGElement | null = null;
  private preview: SVGPathElement | null = null;
  private sim: Simulation<SimNode, SimLink> | null = null;
  private zoomBehavior: ZoomBehavior<SVGSVGElement, unknown> | null = null;
  private simNodes = new Map<string, SimNode>();
  private nodeEls = new Map<string, SVGGElement>();
  private linkEls: LinkElement[] = [];
  private bound = new WeakSet<Element>();
  private ghostPositions = new Map<string, Point>();
  private cancelZoomAnimation: (() => void) | null = null;
  private fitted = false;

  setCallbacks(callbacks: EngineCallbacks): void {
    this.callbacks = callbacks;
  }

  /** Creates the simulation and zoom behaviour; returns the teardown. */
  mount(
    svg: SVGSVGElement,
    viewport: SVGGElement,
    preview: SVGPathElement,
    initial: ViewTransform | undefined,
  ): () => void {
    this.svg = svg;
    this.viewport = viewport;
    this.preview = preview;

    this.sim = forceSimulation<SimNode, SimLink>()
      .force('charge', forceManyBody<SimNode>().strength(-220).distanceMax(520))
      .force('collide', forceCollide<SimNode>(NODE_WIDTH * 0.36))
      .force(
        'link',
        forceLink<SimNode, SimLink>()
          .id((d) => d.id)
          .distance(220)
          .strength(0.08),
      )
      .force('x', forceX<SimNode>((d) => d.anchorX).strength(ANCHOR_STRENGTH))
      .force('y', forceY<SimNode>((d) => d.anchorY).strength(ANCHOR_STRENGTH))
      .alphaDecay(0.045)
      .on('tick', () => this.render());

    const zoomBehavior = zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.08, 2.5])
      .filter((event: MouseEvent) => (!event.ctrlKey || event.type === 'wheel') && !event.button)
      .on('zoom', (event: { transform: { toString(): string } }) => {
        viewport.setAttribute('transform', event.transform.toString());
      })
      .on('end', (event: { transform: ViewTransform }) => {
        const { x, y, k } = event.transform;
        this.callbacks?.onTransformEnd({ x, y, k });
      });
    this.zoomBehavior = zoomBehavior;
    const selection = select(svg).call(zoomBehavior).on('dblclick.zoom', null);
    if (initial) {
      zoomBehavior.transform(
        selection,
        zoomIdentity.translate(initial.x, initial.y).scale(initial.k),
      );
      this.fitted = true;
    }

    // The canvas shares the row with panels that open beside it (the assistant, on the right).
    // Whenever its box changes the whole graph is fitted to the new box, frame by frame as a
    // panel slides, so no node is ever left under it.
    let box = svg.getBoundingClientRect();
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(() => {
            const next = svg.getBoundingClientRect();
            const changed = next.width !== box.width || next.height !== box.height;
            box = next;
            if (changed && next.width > 0 && next.height > 0) this.fitToView(false);
          });
    observer?.observe(svg);

    return () => {
      observer?.disconnect();
      this.cancelZoomAnimation?.();
      this.sim?.stop();
      selection.on('.zoom', null);
      this.sim = null;
      this.zoomBehavior = null;
    };
  }

  /** Syncs server data into the simulation, keeping positions/velocities of existing nodes. */
  syncData(nodes: GraphNode[], edges: GraphEdge[]): void {
    const sim = this.sim;
    if (!sim) return;
    const previous = this.simNodes;
    const next = new Map<string, SimNode>();
    let structureChanged = previous.size !== nodes.length;
    let anchorsMoved = false;

    for (const node of nodes) {
      const existing = previous.get(node.id);
      if (existing) {
        // Ignore echoes for a node being dragged (it is pinned via fx/fy) to avoid reheating.
        const moved = existing.anchorX !== node.x || existing.anchorY !== node.y;
        if (moved && existing.fx == null) anchorsMoved = true;
        existing.anchorX = node.x;
        existing.anchorY = node.y;
        next.set(node.id, existing);
      } else {
        structureChanged = true;
        next.set(node.id, { id: node.id, x: node.x, y: node.y, anchorX: node.x, anchorY: node.y });
      }
    }
    this.simNodes = next;

    const links: SimLink[] = edges
      .filter((e) => next.has(e.source) && next.has(e.target))
      .map((e) => ({ source: e.source, target: e.target }));
    sim.nodes([...next.values()]);
    sim.force<ForceLink<SimNode, SimLink>>('link')?.links(links);

    if (structureChanged) sim.alpha(0.35).restart();
    else if (anchorsMoved) sim.alpha(0.6).restart();

    if (!this.fitted && next.size > 0) {
      this.fitted = true;
      sim.tick(30); // let the layout settle a little before framing it
      this.fitToView(false);
    }
  }

  /** Indexes the DOM React rendered, binds drag to new nodes and places everything. */
  indexElements(ghostPositions: Map<string, Point>): void {
    const viewport = this.viewport;
    if (!viewport) return;
    this.ghostPositions = ghostPositions;

    const nodes = new Map<string, SVGGElement>();
    for (const el of viewport.querySelectorAll<SVGGElement>('[data-node-id]')) {
      if (el.dataset.nodeId) nodes.set(el.dataset.nodeId, el);
    }
    this.nodeEls = nodes;
    this.linkEls = [...viewport.querySelectorAll<SVGGElement>('[data-link-source]')].map((el) => ({
      source: el.dataset.linkSource ?? '',
      target: el.dataset.linkTarget ?? '',
      paths: [...el.querySelectorAll('path')],
    }));

    const unbound = [...nodes.values()].filter((el) => !this.bound.has(el));
    if (unbound.length > 0) {
      selectAll<SVGGElement, unknown>(unbound).call(this.createDrag());
      for (const el of unbound) this.bound.add(el);
    }
    this.render();
  }

  fitToView(animate = true): void {
    const { svg, zoomBehavior } = this;
    if (!svg || !zoomBehavior) return;
    const points = [...this.simNodes.values()].map((n) => ({ x: n.x ?? 0, y: n.y ?? 0 }));
    points.push(...this.ghostPositions.values());
    const bounds = boundsOf(points);
    const { width, height } = svg.getBoundingClientRect();
    if (!bounds || width === 0 || height === 0) return;
    const bw = bounds.maxX - bounds.minX + NODE_WIDTH + 160;
    const bh = bounds.maxY - bounds.minY + NODE_HEIGHT + 160;
    const k = Math.max(0.15, Math.min(1.25, Math.min(width / bw, height / bh)));
    const cx = (bounds.minX + bounds.maxX) / 2;
    const cy = (bounds.minY + bounds.maxY) / 2;
    const target = zoomIdentity.translate(width / 2 - cx * k, height / 2 - cy * k).scale(k);
    const selection = select(svg);
    this.cancelZoomAnimation?.();
    if (!animate || prefersReducedMotion()) {
      zoomBehavior.transform(selection, target);
      return;
    }
    this.cancelZoomAnimation = animateZoom(zoomTransform(svg), target, { width, height }, (t) =>
      zoomBehavior.transform(selection, t),
    );
  }

  viewportCenter(): Point {
    const svg = this.svg;
    if (!svg) return { x: 0, y: 0 };
    const { width, height } = svg.getBoundingClientRect();
    const [x, y] = zoomTransform(svg).invert([width / 2, height / 2]);
    return { x, y };
  }

  private positionOf(key: string): Point | undefined {
    const n = this.simNodes.get(key);
    if (n) return { x: n.x ?? n.anchorX, y: n.y ?? n.anchorY };
    return this.ghostPositions.get(key);
  }

  private render(): void {
    for (const [id, el] of this.nodeEls) {
      const p = this.positionOf(id);
      if (p)
        el.setAttribute('transform', `translate(${p.x - NODE_WIDTH / 2},${p.y - NODE_HEIGHT / 2})`);
    }
    for (const link of this.linkEls) {
      const a = this.positionOf(link.source);
      const b = this.positionOf(link.target);
      if (!a || !b) continue;
      const d = edgePath(a, b);
      for (const path of link.paths) path.setAttribute('d', d);
    }
  }

  /** Drag moves a node; shift-drag draws a link to another node instead. */
  private createDrag() {
    let mode: 'move' | 'connect' = 'move';
    let nodeId = '';
    let moved = false;
    let highlighted: SVGGElement | null = null;

    const highlight = (id: string | null) => {
      const el = id && id !== nodeId ? (this.nodeEls.get(id) ?? null) : null;
      if (el === highlighted) return;
      highlighted?.removeAttribute('data-connect-target');
      el?.setAttribute('data-connect-target', 'true');
      highlighted = el;
    };
    const simNode = (id: string | undefined) => this.simNodes.get(id ?? '');

    return drag<SVGGElement, unknown, Point>()
      .container(() => this.viewport ?? document.body)
      .clickDistance(4)
      .subject(function subject() {
        const n = simNode(this.dataset.nodeId);
        return n ? { x: n.x ?? n.anchorX, y: n.y ?? n.anchorY } : { x: 0, y: 0 };
      })
      .on('start', function start(event: DragEvent) {
        nodeId = this.dataset.nodeId ?? '';
        mode = event.sourceEvent?.shiftKey ? 'connect' : 'move';
        moved = false;
      })
      .on('drag', (event: DragEvent) => {
        const n = this.simNodes.get(nodeId);
        if (!n) return;
        if (mode === 'connect') {
          this.preview?.setAttribute('d', previewPath({ x: n.x ?? 0, y: n.y ?? 0 }, event));
          this.preview?.removeAttribute('visibility');
          highlight(nodeIdAt(event.sourceEvent));
          return;
        }
        if (!moved) {
          moved = true;
          this.sim?.alphaTarget(0.12).restart();
        }
        n.fx = event.x;
        n.fy = event.y;
        this.callbacks?.onMoveNode(nodeId, event.x, event.y, false);
      })
      .on('end', (event: DragEvent) => {
        if (mode === 'connect') {
          this.preview?.setAttribute('visibility', 'hidden');
          highlight(null);
          const target = nodeIdAt(event.sourceEvent);
          if (target && target !== nodeId) this.callbacks?.onConnect(nodeId, target);
          return;
        }
        const n = this.simNodes.get(nodeId);
        if (!n || !moved) return;
        n.anchorX = event.x;
        n.anchorY = event.y;
        n.fx = null;
        n.fy = null;
        this.sim?.alphaTarget(0);
        this.callbacks?.onMoveNode(nodeId, event.x, event.y, true);
      });
  }
}
