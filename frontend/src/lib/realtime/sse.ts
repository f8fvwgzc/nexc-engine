import { buildUrl } from '@/lib/api/client';
import { SSE_EVENT_TYPES, type SseEvent } from '@/schemas/realtime';

import { backoffDelay } from './backoff';
import type { ConnectionState } from './connection-state';
import { parseSseEvent } from './sse-parser';
import { requestRealtimeTicket } from './ticket';

export interface GraphEventStreamOptions {
  onEvent: (event: SseEvent) => void;
  onStateChange?: (state: ConnectionState) => void;
  /** Called after a reconnect so the caller can resync anything missed while offline. */
  onReconnect?: () => void;
  onInvalidFrame?: (type: string, raw: string) => void;
}

/**
 * EventSource for `GET /graphs/{gid}/events`. Tickets are single-use, so the browser's built-in
 * auto-reconnect (which would replay the consumed ticket) is replaced by our own: close, fetch a
 * fresh ticket, reconnect with exponential backoff.
 */
export class GraphEventStream {
  private source: EventSource | null = null;
  private attempt = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private abort: AbortController | null = null;
  private stopped = false;
  private hasConnected = false;

  private readonly graphId: string;
  private readonly opts: GraphEventStreamOptions;

  constructor(graphId: string, opts: GraphEventStreamOptions) {
    this.graphId = graphId;
    this.opts = opts;
  }

  start(): void {
    this.stopped = false;
    void this.connect();
  }

  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.abort?.abort();
    this.source?.close();
    this.source = null;
    this.opts.onStateChange?.('closed');
  }

  private async connect(): Promise<void> {
    this.opts.onStateChange?.(this.hasConnected ? 'reconnecting' : 'connecting');
    this.abort = new AbortController();
    let ticket: string;
    try {
      ({ ticket } = await requestRealtimeTicket(this.graphId, this.abort.signal));
    } catch {
      this.scheduleReconnect();
      return;
    }
    if (this.stopped) return;

    const source = new EventSource(buildUrl(`/graphs/${this.graphId}/events`, { ticket }));
    this.source = source;
    source.onopen = () => {
      const isReconnect = this.hasConnected;
      this.attempt = 0;
      this.hasConnected = true;
      this.opts.onStateChange?.('open');
      if (isReconnect) this.opts.onReconnect?.();
    };
    source.onerror = () => {
      source.close();
      if (this.source === source) this.source = null;
      this.scheduleReconnect();
    };
    for (const type of SSE_EVENT_TYPES) {
      source.addEventListener(type, (message: MessageEvent<string>) => {
        const event = parseSseEvent(type, message.data, message.lastEventId || null);
        if (event) this.opts.onEvent(event);
        else this.opts.onInvalidFrame?.(type, message.data);
      });
    }
  }

  private scheduleReconnect(): void {
    if (this.stopped) return;
    this.opts.onStateChange?.('reconnecting');
    const delay = backoffDelay(this.attempt++);
    this.timer = setTimeout(() => void this.connect(), delay);
  }
}
