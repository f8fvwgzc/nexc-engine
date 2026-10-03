import { apiWebSocketUrl } from '@/lib/env';
import {
  wsServerMessageSchema,
  type Cursor,
  type WsClientMessage,
  type WsServerMessage,
} from '@/schemas/realtime';

import { backoffDelay } from './backoff';
import type { ConnectionState } from './connection-state';
import { keyedThrottle } from './throttle';
import { requestRealtimeTicket } from './ticket';

const HEARTBEAT_MS = 20_000;
const PONG_TIMEOUT_MS = 10_000;
const MOVE_THROTTLE_MS = 80;
const PRESENCE_THROTTLE_MS = 100;

export interface GraphSocketOptions {
  onMessage: (message: WsServerMessage) => void;
  onStateChange?: (state: ConnectionState) => void;
  onReconnect?: () => void;
}

/**
 * WebSocket for `GET /graphs/{gid}/ws` (CONTRACT §7): ticket auth, heartbeat ping/pong,
 * reconnect with backoff, throttled `node.move` / `presence` sends.
 */
export class GraphSocket {
  private ws: WebSocket | null = null;
  private attempt = 0;
  private stopped = false;
  private hasConnected = false;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private pongTimer: ReturnType<typeof setTimeout> | null = null;
  private abort: AbortController | null = null;

  private readonly moves = keyedThrottle(MOVE_THROTTLE_MS, (nodeId, x: number, y: number) =>
    this.send({ type: 'node.move', node_id: nodeId, x, y }),
  );
  private readonly presence = keyedThrottle(PRESENCE_THROTTLE_MS, (_key, cursor: Cursor) =>
    this.send({ type: 'presence', cursor }),
  );

  private readonly graphId: string;
  private readonly opts: GraphSocketOptions;

  constructor(graphId: string, opts: GraphSocketOptions) {
    this.graphId = graphId;
    this.opts = opts;
  }

  get isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  start(): void {
    this.stopped = false;
    void this.connect();
  }

  stop(): void {
    this.stopped = true;
    this.moves.flush();
    this.presence.cancel();
    this.clearTimers();
    this.abort?.abort();
    this.ws?.close(1000, 'client closing');
    this.ws = null;
    this.opts.onStateChange?.('closed');
  }

  /** Persisted position update; throttled per node, last position wins. */
  moveNode(nodeId: string, x: number, y: number): void {
    this.moves.call(nodeId, Math.round(x), Math.round(y));
  }

  /** Flushes any throttled moves immediately (e.g. on drag end). */
  flushMoves(): void {
    this.moves.flush();
  }

  sendPresence(cursor: Cursor): void {
    this.presence.call('cursor', cursor);
  }

  private send(message: WsClientMessage): void {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(message));
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

    const url = new URL(apiWebSocketUrl(`/graphs/${this.graphId}/ws`));
    url.searchParams.set('ticket', ticket);
    const ws = new WebSocket(url);
    this.ws = ws;

    ws.onopen = () => {
      const isReconnect = this.hasConnected;
      this.attempt = 0;
      this.hasConnected = true;
      this.startHeartbeat();
      this.opts.onStateChange?.('open');
      if (isReconnect) this.opts.onReconnect?.();
    };
    ws.onmessage = (event: MessageEvent) => {
      if (typeof event.data !== 'string') return;
      const message = this.parse(event.data);
      if (!message) return;
      if (message.type === 'pong') {
        if (this.pongTimer) clearTimeout(this.pongTimer);
        this.pongTimer = null;
        return;
      }
      this.opts.onMessage(message);
    };
    ws.onclose = () => {
      if (this.ws === ws) this.ws = null;
      this.clearTimers();
      this.scheduleReconnect();
    };
  }

  private parse(raw: string): WsServerMessage | null {
    try {
      const result = wsServerMessageSchema.safeParse(JSON.parse(raw));
      return result.success ? result.data : null;
    } catch {
      return null;
    }
  }

  private startHeartbeat(): void {
    this.heartbeatTimer = setInterval(() => {
      this.send({ type: 'ping' });
      this.pongTimer ??= setTimeout(
        () => this.ws?.close(4000, 'heartbeat timeout'),
        PONG_TIMEOUT_MS,
      );
    }, HEARTBEAT_MS);
  }

  private clearTimers(): void {
    if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
    if (this.pongTimer) clearTimeout(this.pongTimer);
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.heartbeatTimer = null;
    this.pongTimer = null;
    this.reconnectTimer = null;
  }

  private scheduleReconnect(): void {
    if (this.stopped) return;
    this.opts.onStateChange?.('reconnecting');
    this.reconnectTimer = setTimeout(() => void this.connect(), backoffDelay(this.attempt++));
  }
}
