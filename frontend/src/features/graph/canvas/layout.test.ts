import { describe, expect, it } from 'vitest';

import { clipToCard, edgePath, NODE_HEIGHT, NODE_WIDTH } from './geometry';
import { boundsOf, layoutGhosts, layoutLevels } from './layout';

describe('layoutLevels', () => {
  it('places each topological level in its own column, vertically centered', () => {
    const positions = layoutLevels([['a'], ['b', 'c'], ['d']], { columnGap: 200, rowGap: 100 });
    expect(positions.get('a')).toEqual({ x: 0, y: 0 });
    expect(positions.get('b')).toEqual({ x: 200, y: -50 });
    expect(positions.get('c')).toEqual({ x: 200, y: 50 });
    expect(positions.get('d')).toEqual({ x: 400, y: 0 });
  });

  it('respects the origin and keeps the first position for duplicate ids', () => {
    const positions = layoutLevels(
      [
        ['a', 'b'],
        ['a', 'c'],
      ],
      {
        columnGap: 100,
        rowGap: 10,
        origin: { x: 50, y: 50 },
      },
    );
    expect(positions.get('a')).toEqual({ x: 50, y: 45 });
    expect(positions.get('c')).toEqual({ x: 150, y: 50 });
    expect(positions.size).toBe(3);
  });

  it('returns nothing for an empty graph', () => {
    expect(layoutLevels([]).size).toBe(0);
  });
});

describe('layoutGhosts', () => {
  const existing = new Map([
    ['n1', { x: 0, y: 0 }],
    ['n2', { x: 300, y: 200 }],
  ]);
  const proposal = (ref: string, existing_id: string | null = null) => ({
    ref,
    existing_id,
    title: ref,
    content: '',
    kind: 'task' as const,
    agent_role: '',
    executor: 'llm' as const,
    tags: [],
  });

  it('overlays refinements on their existing node and stacks new nodes to the right', () => {
    const positions = layoutGhosts(
      [proposal('r1', 'n2'), proposal('r2'), proposal('r3')],
      existing,
      2,
    );
    expect(positions.get('r1')).toEqual({ x: 300, y: 200 });
    const r2 = positions.get('r2')!;
    const r3 = positions.get('r3')!;
    expect(r2.x).toBeGreaterThan(300 + NODE_WIDTH / 2);
    expect(r3.x).toBe(r2.x);
    expect(r3.y).toBeGreaterThan(r2.y);
  });
});

describe('geometry', () => {
  it('computes bounds', () => {
    expect(
      boundsOf([
        { x: 1, y: 5 },
        { x: -2, y: 3 },
      ]),
    ).toEqual({ minX: -2, minY: 3, maxX: 1, maxY: 5 });
    expect(boundsOf([])).toBeNull();
  });

  it('clips edge endpoints to the node card border', () => {
    const p = clipToCard({ x: 0, y: 0 }, { x: 1000, y: 0 }, 0);
    expect(p).toEqual({ x: NODE_WIDTH / 2, y: 0 });
    const q = clipToCard({ x: 0, y: 0 }, { x: 0, y: -1000 }, 0);
    expect(q).toEqual({ x: 0, y: -NODE_HEIGHT / 2 });
  });

  it('draws a quadratic curve between two nodes', () => {
    expect(edgePath({ x: 0, y: 0 }, { x: 400, y: 0 })).toMatch(/^M[\d.,-]+Q[\d.,-]+ [\d.,-]+$/);
  });
});
