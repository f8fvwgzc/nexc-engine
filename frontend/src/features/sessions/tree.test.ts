import { describe, expect, it } from 'vitest';

import type { Rollup, Session, SessionAgent } from '@/schemas/session';
import type { Span } from '@/schemas/trace';

import { allKeys, displayAgent, filterRows, flattenSession, spansOfRow } from './tree';

const WS = '0190a1b2-c3d4-7e5f-8a9b-9c1d2e3f4a5b';
const NODE = '0190a1b2-c3d4-7e5f-8a9b-2c1d2e3f4a5b';
const AGENT = '0190a1b2-c3d4-7e5f-8a9b-8c1d2e3f4a5b';
const id = (n: string) => `0190a1b2-c3d4-7e5f-8a9b-${n.padStart(12, '0')}`;

const rollup = (over: Partial<Rollup> = {}): Rollup => ({
  spans: 1,
  llm_calls: 1,
  tool_calls: 0,
  denials: 0,
  tokens_in: 10,
  tokens_out: 2,
  tokens_cached: 0,
  cost_usd: 0.01,
  ...over,
});

const agent = (principal: string, over: Partial<SessionAgent> = {}): SessionAgent => ({
  principal,
  agent_id: AGENT,
  role: null,
  depth: 0,
  spawn_span_id: null,
  parent_principal: null,
  status: 'succeeded',
  started_at: '2026-10-18T12:00:00Z',
  ended_at: '2026-10-18T12:00:05Z',
  duration_ms: 5000,
  own: rollup(),
  total: rollup(),
  span_ids: [id('1')],
  cancellable: false,
  children: [],
  ...over,
});

const session: Session = {
  run_id: id('ff'),
  graph_id: id('ee'),
  graph_name: 'Research',
  workspace_id: WS,
  status: 'running',
  created_at: '2026-10-18T12:00:00Z',
  started_at: '2026-10-18T12:00:00Z',
  finished_at: null,
  duration_ms: null,
  rollup: rollup({ spans: 4, llm_calls: 3 }),
  by_model: [],
  recording: false,
  nodes: [
    {
      node_id: NODE,
      title: 'Customs',
      executor: 'agent',
      status: 'running',
      attempt: 1,
      cached: false,
      error: null,
      started_at: '2026-10-18T12:00:00Z',
      ended_at: null,
      duration_ms: null,
      rollup: rollup({ spans: 4, llm_calls: 3 }),
      context_span_ids: [id('0')],
      agents: [
        agent(`agent://${WS}/Analyst`, {
          status: 'running',
          total: rollup({ llm_calls: 3, tokens_in: 30 }),
          children: [
            agent(`agent://${WS}/Analyst/researcher-1`, {
              role: 'researcher',
              depth: 1,
              spawn_span_id: id('2'),
              parent_principal: `agent://${WS}/Analyst`,
              status: 'running',
              cancellable: true,
              span_ids: [id('3')],
              children: [
                agent(`agent://${WS}/Analyst/researcher-1/qa-1`, {
                  role: 'qa',
                  depth: 2,
                  spawn_span_id: id('4'),
                  status: 'cancelled',
                  span_ids: [id('5')],
                }),
              ],
            }),
          ],
        }),
      ],
    },
  ],
};

const span = (n: string, kind: Span['kind']): Span => ({
  id: id(n),
  run_id: session.run_id,
  node_id: NODE,
  agent_id: AGENT,
  principal: 'x',
  parent_id: null,
  kind,
  name: kind,
  started_at: '2026-10-18T12:00:00Z',
  ended_at: null,
  status: 'ok',
  attributes: {},
  seq: 0,
  prev_hash: '0'.repeat(64),
  hash: '0'.repeat(64),
});

describe('session tree', () => {
  it('flattens what is expanded, with its ancestors first', () => {
    expect(flattenSession(session, new Set()).map((r) => r.key)).toEqual(['run']);
    const open = flattenSession(session, allKeys(session));
    expect(open.map((r) => [r.kind, r.depth, r.title])).toEqual([
      ['run', 0, `Run ${session.run_id.slice(0, 8)}`],
      ['node', 1, 'Customs'],
      ['agent', 2, 'Analyst'],
      ['agent', 3, 'researcher-1'],
      ['agent', 4, 'qa-1'],
    ]);
    const researcher = open[3]!;
    expect(researcher.cancellable).toBe(id('2'));
    expect(researcher.own).toEqual(rollup());
    expect(researcher.parentKey).toBe(`agent:${NODE}:agent://${WS}/Analyst`);
    expect(open[4]!.cancellable).toBeNull();
    expect(open[4]!.own).toBeNull();
    expect(open[1]!.spanIds).toEqual([id('0')]);
    // Only the run and the node open: the agents stay folded.
    const partly = flattenSession(session, new Set(['run', `node:${NODE}`]));
    expect(partly.map((r) => r.kind)).toEqual(['run', 'node', 'agent']);
    expect(displayAgent('agent://w/A/b-1')).toBe('b-1');
    expect(displayAgent('system')).toBe('system');
  });

  it('narrows by status, kind and agent while keeping ancestors', () => {
    const rows = flattenSession(session, allKeys(session));
    const spans = new Map([
      [id('0'), span('0', 'data_read')],
      [id('1'), span('1', 'llm_call')],
      [id('3'), span('3', 'llm_call')],
      [id('5'), span('5', 'tool_call')],
    ]);
    const cancelled = filterRows(rows, { agent: 'all', kind: 'all', status: 'cancelled' }, spans);
    expect(cancelled.map((r) => r.title)).toEqual([
      `Run ${session.run_id.slice(0, 8)}`,
      'Customs',
      'Analyst',
      'researcher-1',
      'qa-1',
    ]);
    const tools = filterRows(rows, { agent: 'all', kind: 'tool_call', status: 'all' }, spans);
    expect(tools.map((r) => r.title)).toContain('qa-1');
    expect(tools.map((r) => r.title)).not.toContain('Customs' + 'x');
    const nobody = filterRows(rows, { agent: id('99'), kind: 'all', status: 'all' }, spans);
    expect(nobody.map((r) => r.kind)).toEqual(['run', 'node']);
    expect(filterRows(rows, { agent: 'all', kind: 'all', status: 'all' }, spans)).toBe(rows);
    expect(spansOfRow(rows[4]!, [...spans.values()]).map((s) => s.id)).toEqual([id('5')]);
    expect(spansOfRow(rows[0]!, [...spans.values()])).toHaveLength(4);
  });
});
