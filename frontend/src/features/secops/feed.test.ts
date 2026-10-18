import { describe, expect, it } from 'vitest';

import { span } from '@/test/fixtures';
import type { Alert } from '@/schemas/policy';

import { feedLink, parseFeedFrame, pushFeedEvent, type FeedEvent } from './feed';

const alert: Alert = {
  id: '0190a1b2-c3d4-7e5f-8a9b-bc1d2e3f4a5b',
  workspace_id: span.run_id,
  agent_id: span.agent_id,
  principal: 'agent://ws/Researcher',
  run_id: span.run_id,
  node_id: null,
  graph_id: null,
  span_id: span.id,
  kind: 'dlp',
  severity: 'high',
  rule: 'dlp.secrets',
  title: '1 credential in notes.md',
  detail: '',
  created_at: '2026-10-03T12:00:02Z',
  acknowledged_at: null,
  acknowledged_by: null,
  deliveries: 0,
};

describe('parseFeedFrame', () => {
  it('turns span and alert frames into lines and ignores the rest', () => {
    const line = parseFeedFrame({ event: 'trace.span', data: JSON.stringify({ span }) });
    expect(line).toMatchObject({ kind: 'span', id: span.id, at: span.started_at });
    const raised = parseFeedFrame({ event: 'alert', data: JSON.stringify({ alert }) });
    expect(raised).toMatchObject({ kind: 'alert', id: alert.id, at: alert.created_at });
    expect(parseFeedFrame({ event: 'heartbeat', data: '{"at":"now"}' })).toBeNull();
    expect(
      parseFeedFrame({
        event: 'node.status',
        data: JSON.stringify({
          run_id: span.run_id,
          node_id: span.node_id,
          status: 'running',
          attempt: 1,
          error: null,
          cached: false,
        }),
      }),
    ).toBeNull();
    expect(parseFeedFrame({ event: 'trace.span', data: 'not json' })).toBeNull();
    expect(parseFeedFrame({ event: 'trace.span', data: '{"span":{"id":1}}' })).toBeNull();
  });
});

describe('pushFeedEvent', () => {
  const line = (id: string): FeedEvent => ({
    kind: 'span',
    id,
    at: span.started_at,
    span: { ...span, id },
  });

  it('keeps the newest first, drops repeats and the overflow', () => {
    let list: FeedEvent[] = [];
    list = pushFeedEvent(list, line('a'), 2);
    list = pushFeedEvent(list, line('b'), 2);
    expect(list.map((e) => e.id)).toEqual(['b', 'a']);
    expect(pushFeedEvent(list, line('b'), 2)).toBe(list);
    list = pushFeedEvent(list, line('c'), 2);
    expect(list.map((e) => e.id)).toEqual(['c', 'b']);
  });

  it('links a line to its run at its span', () => {
    expect(feedLink(line('x'))).toBe(`/app/runs/${span.run_id}?span=x`);
    const raised: FeedEvent = { kind: 'alert', id: alert.id, at: alert.created_at, alert };
    expect(feedLink(raised)).toBe(`/app/runs/${span.run_id}?span=${span.id}`);
    expect(feedLink({ ...raised, alert: { ...alert, run_id: null } })).toBeNull();
  });
});
