import { useCallback, useEffect, useRef, useState } from 'react';

import { backoffDelay } from '@/lib/realtime/backoff';
import type { ConnectionState } from '@/lib/realtime/connection-state';

import { readSecopsFeed } from '../api';
import { pushFeedEvent, type FeedEvent } from '../feed';

export interface SecopsFeed {
  /** Newest first. */
  events: FeedEvent[];
  state: ConnectionState;
  /** Lines that arrived while paused, waiting to be shown. */
  held: number;
  clear: () => void;
}

/**
 * Follows the workspace's live feed while mounted: every span and alert of every graph the
 * member may open, newest first, at most `FEED_MAX` lines. The connection is re-opened with
 * backoff when it drops. While `paused`, lines keep arriving but are held back, so the list
 * stays still under the reader's eyes; unpausing shows them.
 */
export function useSecopsFeed(workspaceId: string | undefined, paused: boolean): SecopsFeed {
  const [events, setEvents] = useState<FeedEvent[]>([]);
  const [state, setState] = useState<ConnectionState>('closed');
  const [held, setHeld] = useState(0);
  const pending = useRef<FeedEvent[]>([]);
  const pausedRef = useRef(paused);
  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

  const flush = useCallback(() => {
    const waiting = pending.current;
    pending.current = [];
    if (waiting.length === 0) return;
    setEvents((list) => waiting.reduce((acc, event) => pushFeedEvent(acc, event), list));
    setHeld(0);
  }, []);

  useEffect(() => {
    if (!paused) flush();
  }, [paused, flush]);

  useEffect(() => {
    if (!workspaceId) return;
    const abort = new AbortController();
    let attempt = 0;
    let connected = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onEvent = (event: FeedEvent) => {
      if (pausedRef.current) {
        pending.current = [event, ...pending.current];
        setHeld(pending.current.length);
      } else {
        setEvents((list) => pushFeedEvent(list, event));
      }
    };
    const connect = () => {
      setState(connected ? 'reconnecting' : 'connecting');
      readSecopsFeed(workspaceId, onEvent, abort.signal, () => {
        connected = true;
        attempt = 0;
        setState('open');
      })
        .catch(() => undefined)
        .finally(() => {
          if (abort.signal.aborted) return;
          setState('reconnecting');
          timer = setTimeout(connect, backoffDelay(attempt++));
        });
    };
    connect();
    return () => {
      abort.abort();
      if (timer) clearTimeout(timer);
      setState('closed');
    };
  }, [workspaceId]);

  const clear = useCallback(() => {
    pending.current = [];
    setHeld(0);
    setEvents([]);
  }, []);

  return { events, state, held, clear };
}
