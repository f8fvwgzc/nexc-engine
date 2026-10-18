import { parseSseEvent } from '@/lib/realtime/sse-parser';
import type { Alert } from '@/schemas/policy';
import type { Span, SpanKind } from '@/schemas/trace';

import type { SseFrame } from '../audit/sse-frames';

/** One line of the console's live feed: a span or an alert, as it arrived. */
export type FeedEvent =
  | { kind: 'span'; id: string; at: string; span: Span }
  | { kind: 'alert'; id: string; at: string; alert: Alert };

/** Most lines the feed keeps; older ones fall off the end. */
export const FEED_MAX = 200;

/** Turns a frame of the workspace feed into a line, or nothing for heartbeats and the rest. */
export function parseFeedFrame(frame: SseFrame): FeedEvent | null {
  const event = parseSseEvent(frame.event, frame.data);
  if (!event) return null;
  if (event.type === 'trace.span') {
    const { span } = event.data;
    return { kind: 'span', id: span.id, at: span.started_at, span };
  }
  if (event.type === 'alert') {
    const { alert } = event.data;
    return { kind: 'alert', id: alert.id, at: alert.created_at, alert };
  }
  return null;
}

/** Puts `event` at the head of `list` (newest first), dropping a repeat and the overflow. */
export function pushFeedEvent(list: FeedEvent[], event: FeedEvent, max = FEED_MAX): FeedEvent[] {
  if (list.some((e) => e.kind === event.kind && e.id === event.id)) return list;
  const next = [event, ...list];
  return next.length > max ? next.slice(0, max) : next;
}

/** The agent a line is about, as its id; `null` for the system's own spans. */
export function feedAgentId(event: FeedEvent): string | null {
  return event.kind === 'span' ? event.span.agent_id : event.alert.agent_id;
}

/** The run a line belongs to, if any. */
export function feedRunId(event: FeedEvent): string | null {
  return event.kind === 'span' ? event.span.run_id : event.alert.run_id;
}

/** Where a click on a line goes: the run, at the span when there is one. */
export function feedLink(event: FeedEvent): string | null {
  if (event.kind === 'span') return `/app/runs/${event.span.run_id}?span=${event.span.id}`;
  if (!event.alert.run_id) return null;
  const at = event.alert.span_id ? `?span=${event.alert.span_id}` : '';
  return `/app/runs/${event.alert.run_id}${at}`;
}

/** What the feed is narrowed to: everything, alerts only, or one kind of span. */
export type KindFilter = 'all' | 'alert' | SpanKind;

/** Lines of `kind` about `agent` (`all` for every agent). */
export function filterFeed(events: FeedEvent[], kind: KindFilter, agent: string): FeedEvent[] {
  return events.filter((event) => {
    if (kind === 'alert' && event.kind !== 'alert') return false;
    if (kind !== 'all' && kind !== 'alert' && (event.kind !== 'span' || event.span.kind !== kind)) {
      return false;
    }
    return agent === 'all' || feedAgentId(event) === agent;
  });
}
