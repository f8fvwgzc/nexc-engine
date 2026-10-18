import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';

import { mergeSpan } from '@/features/runs/api';
import { qk } from '@/lib/query-keys';
import { GraphEventStream } from '@/lib/realtime/sse';
import { isRunActive } from '@/schemas/run';
import type { Session } from '@/schemas/session';
import type { Span } from '@/schemas/trace';

/** How long arriving events are gathered before the tree is read again. */
const REFRESH_AFTER_MS = 400;

/**
 * While the run is live, follows its graph's event stream: every `trace.span` of the run joins
 * the trace query at once and, with every `node.status`, schedules the tree to be read again
 * (gathered, so a burst of spans is one request). When the run ends, the tree and the trace are
 * read once more so nothing that arrived while reconnecting is missed.
 */
export function useLiveSession(session: Session | undefined) {
  const queryClient = useQueryClient();
  const runId = session?.run_id;
  const graphId = session?.graph_id;
  const active = session !== undefined && isRunActive(session.status);
  const wasActive = useRef(active);

  useEffect(() => {
    if (runId && wasActive.current && !active) {
      void queryClient.invalidateQueries({ queryKey: qk.runs.trace(runId) });
      void queryClient.invalidateQueries({ queryKey: qk.sessions.detail(runId) });
    }
    wasActive.current = active;
  }, [active, runId, queryClient]);

  useEffect(() => {
    if (!active || !runId || !graphId) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const refresh = () => {
      if (timer) return;
      timer = setTimeout(() => {
        timer = null;
        void queryClient.invalidateQueries({ queryKey: qk.sessions.detail(runId) });
      }, REFRESH_AFTER_MS);
    };
    const stream = new GraphEventStream(graphId, {
      onEvent: (event) => {
        if (event.type === 'trace.span' && event.data.span.run_id === runId) {
          const { span } = event.data;
          queryClient.setQueryData<Span[]>(qk.runs.trace(runId), (spans) => mergeSpan(spans, span));
          refresh();
        } else if (event.type === 'node.status' && event.data.run_id === runId) {
          refresh();
        } else if (event.type === 'run.finished' && event.data.run.id === runId) {
          queryClient.setQueryData(qk.runs.detail(runId), event.data.run);
          void queryClient.invalidateQueries({ queryKey: qk.sessions.detail(runId) });
          void queryClient.invalidateQueries({ queryKey: qk.runs.trace(runId) });
        }
      },
      onReconnect: () => {
        void queryClient.invalidateQueries({ queryKey: qk.sessions.detail(runId) });
        void queryClient.invalidateQueries({ queryKey: qk.runs.trace(runId) });
      },
    });
    stream.start();
    return () => {
      stream.stop();
      if (timer) clearTimeout(timer);
    };
  }, [active, graphId, runId, queryClient]);
}
