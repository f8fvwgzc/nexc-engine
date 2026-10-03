import { describe, expect, it } from 'vitest';

import * as f from '@/test/fixtures';

import { parseSseEvent } from './sse-parser';

describe('parseSseEvent (CONTRACT §6)', () => {
  it('returns a typed event for a valid frame', () => {
    const event = parseSseEvent(
      'node.status',
      JSON.stringify({
        run_id: f.ids.run,
        node_id: f.ids.nodeA,
        status: 'running',
        attempt: 1,
        error: null,
        cached: false,
      }),
      '17',
    );
    expect(event).toEqual({
      type: 'node.status',
      id: '17',
      data: {
        run_id: f.ids.run,
        node_id: f.ids.nodeA,
        status: 'running',
        attempt: 1,
        error: null,
        cached: false,
      },
    });
  });

  it('validates nested contract objects (plan.node, run.finished, artifact.created)', () => {
    expect(
      parseSseEvent('plan.node', JSON.stringify({ plan_id: f.ids.plan, node: f.plan.nodes[0] }))
        ?.type,
    ).toBe('plan.node');
    expect(
      parseSseEvent('run.finished', JSON.stringify({ run: { ...f.run, status: 'succeeded' } }))
        ?.type,
    ).toBe('run.finished');
    expect(parseSseEvent('artifact.created', JSON.stringify({ artifact: f.artifact }))?.type).toBe(
      'artifact.created',
    );
  });

  it('ignores unknown event names', () => {
    expect(parseSseEvent('node.teleported', '{}')).toBeNull();
    expect(parseSseEvent('toString', '{}')).toBeNull();
  });

  it('rejects malformed JSON and payloads that break the schema', () => {
    expect(parseSseEvent('heartbeat', '{not json')).toBeNull();
    expect(
      parseSseEvent('node.output', JSON.stringify({ run_id: f.ids.run, node_id: f.ids.nodeA })),
    ).toBeNull();
    expect(
      parseSseEvent(
        'node.status',
        JSON.stringify({
          run_id: f.ids.run,
          node_id: f.ids.nodeA,
          status: 'cached',
          attempt: 1,
          error: null,
          cached: true,
        }),
      ),
    ).toBeNull();
  });
});
