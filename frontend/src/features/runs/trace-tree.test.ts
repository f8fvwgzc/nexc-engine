import { describe, expect, it } from 'vitest';

import type { Span } from '@/schemas/trace';

import { barOf, buildTraceTree, describeSpan, traceWindow } from './trace-tree';

const NODE_A = '0190a1b2-c3d4-7e5f-8a9b-2c1d2e3f4a5b';
const NODE_B = '0190a1b2-c3d4-7e5f-8a9b-3c1d2e3f4a5b';
let seq = 0;

function span(id: string, over: Partial<Span>): Span {
  const n = seq++;
  return {
    id: `0190a1b2-c3d4-7e5f-8a9b-${id.padStart(12, '0')}`,
    run_id: '0190a1b2-c3d4-7e5f-8a9b-6c1d2e3f4a5b',
    node_id: NODE_A,
    agent_id: null,
    principal: 'Researcher',
    parent_id: null,
    kind: 'tool_call',
    name: 'write_file',
    started_at: `2026-10-17T12:00:${String(10 + n).padStart(2, '0')}Z`,
    ended_at: `2026-10-17T12:00:${String(11 + n).padStart(2, '0')}Z`,
    status: 'ok',
    attributes: {},
    seq: n,
    prev_hash: '0'.repeat(64),
    hash: 'a'.repeat(64),
    ...over,
  };
}

const full = (id: string) => `0190a1b2-c3d4-7e5f-8a9b-${id.padStart(12, '0')}`;

describe('trace tree', () => {
  it('groups node → agent → turn → tool call', () => {
    seq = 0;
    const spans = [
      span('1', { kind: 'data_read', name: 'context', principal: 'Researcher' }),
      span('2', { kind: 'llm_call', name: 'claude', attributes: { turn: 1 } }),
      span('3', { parent_id: full('2') }),
      span('4', { kind: 'spawn', name: 'Researcher/qa-1', parent_id: full('2') }),
      span('5', { kind: 'llm_call', principal: 'Researcher/qa-1', parent_id: full('4') }),
      span('6', { parent_id: full('5'), principal: 'Researcher/qa-1', status: 'error' }),
      span('7', { kind: 'llm_call', node_id: NODE_B, principal: 'Writer' }),
      span('8', { parent_id: 'not-a-known-span' }),
    ];
    const [a, b] = buildTraceTree(spans);
    expect(a!.nodeId).toBe(NODE_A);
    expect(b!.nodeId).toBe(NODE_B);
    expect(a!.context.map((s) => s.name)).toEqual(['context']);
    expect(a!.spans).toBe(7);
    expect(a!.errors).toBe(1);
    const [root, child] = a!.agents;
    expect(root!.principal).toBe('Researcher');
    expect(root!.rows.map((r) => r.span.kind)).toEqual(['llm_call', 'tool_call']);
    expect(root!.rows[0]!.children.map((r) => r.span.kind)).toEqual(['tool_call', 'spawn']);
    expect(child!.principal).toBe('Researcher/qa-1');
    expect(child!.rows[0]!.children[0]!.span.status).toBe('error');
    expect(b!.agents[0]!.rows).toHaveLength(1);
  });

  it('draws bars inside the window and describes spans in words', () => {
    seq = 0;
    const spans = [span('1', {}), span('2', {}), span('3', { ended_at: null })];
    const window = traceWindow(spans);
    const first = barOf(spans[0]!, window);
    expect(first.left).toBe(0);
    expect(first.width).toBe(50);
    expect(barOf(spans[2]!, window)).toEqual({ left: 99.5, width: 0.5 });
    expect(traceWindow([])).toEqual({ start: 0, end: 0 });
    expect(
      describeSpan(
        span('9', { kind: 'llm_call', attributes: { turn: 2, tokens_in: 10, tokens_out: 3 } }),
      ),
    ).toBe('turn 2 · 10 in / 3 out');
    expect(
      describeSpan(
        span('9', {
          kind: 'llm_call',
          attributes: { turn: 3, evidence: 'partial', evidence_coverage: 50 },
        }),
      ),
    ).toBe('turn 3 · evidence partial 50%');
    expect(
      describeSpan(
        span('9', { kind: 'data_read', attributes: { document_chunk_ids: ['x'], memory_ids: [] } }),
      ),
    ).toBe('1 passages · 0 memories · 0 upstream');
  });
});
