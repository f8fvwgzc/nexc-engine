import type { ProposedNode } from '@/schemas/plan';

import { NODE_HEIGHT, NODE_WIDTH, type Point } from './geometry';

export interface LevelLayoutOptions {
  /** Horizontal distance between level columns (center to center). */
  columnGap?: number;
  /** Vertical distance between nodes within a column (center to center). */
  rowGap?: number;
  origin?: Point;
}

/**
 * Turns topological levels (GET /analysis `levels`) into left-to-right column coordinates.
 * Each column is vertically centered on `origin.y`; duplicate ids keep their first position.
 */
export function layoutLevels(
  levels: string[][],
  {
    columnGap = NODE_WIDTH + 96,
    rowGap = NODE_HEIGHT + 40,
    origin = { x: 0, y: 0 },
  }: LevelLayoutOptions = {},
): Map<string, Point> {
  const positions = new Map<string, Point>();
  levels.forEach((level, column) => {
    const ids = level.filter((id) => !positions.has(id));
    const top = origin.y - ((ids.length - 1) * rowGap) / 2;
    ids.forEach((id, row) => {
      positions.set(id, { x: origin.x + column * columnGap, y: top + row * rowGap });
    });
  });
  return positions;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export function boundsOf(points: Iterable<Point>): Bounds | null {
  let bounds: Bounds | null = null;
  for (const p of points) {
    bounds = bounds
      ? {
          minX: Math.min(bounds.minX, p.x),
          minY: Math.min(bounds.minY, p.y),
          maxX: Math.max(bounds.maxX, p.x),
          maxY: Math.max(bounds.maxY, p.y),
        }
      : { minX: p.x, minY: p.y, maxX: p.x, maxY: p.y };
  }
  return bounds;
}

/**
 * Positions for streamed plan proposals: nodes that refine an existing node sit on top of it;
 * new ones fill columns to the right of the current graph, in arrival order.
 */
export function layoutGhosts(
  proposed: ProposedNode[],
  existing: Map<string, Point>,
  perColumn = 6,
): Map<string, Point> {
  const bounds = boundsOf(existing.values()) ?? { minX: 0, minY: 0, maxX: -NODE_WIDTH, maxY: 0 };
  const startX = bounds.maxX + NODE_WIDTH + 80;
  const positions = new Map<string, Point>();
  let index = 0;
  for (const node of proposed) {
    const anchor = node.existing_id ? existing.get(node.existing_id) : undefined;
    if (anchor) {
      positions.set(node.ref, anchor);
      continue;
    }
    positions.set(node.ref, {
      x: startX + Math.floor(index / perColumn) * (NODE_WIDTH + 48),
      y: bounds.minY + (index % perColumn) * (NODE_HEIGHT + 36),
    });
    index += 1;
  }
  return positions;
}
