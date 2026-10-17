import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';

import { qk } from '@/lib/query-keys';
import { GraphEventStream } from '@/lib/realtime/sse';
import { isRunActive, type Run } from '@/schemas/run';
import type { Span } from '@/schemas/trace';

import { mergeSpan } from '../api';

/**
 * While `run` is live, follows its graph's event stream and adds each `trace.span` of the run to
 * the trace query as it arrives. When the run ends the trace and its verification are read again,
 * so nothing that arrived while the stream was reconnecting is missed.
 */
export function useLiveTrace(run: Run) {
  const queryClient = useQueryClient();
  const active = isRunActive(run.status);
  const { id: runId, graph_id: graphId } = run;
  const wasActive = useRef(active);

  useEffect(() => {
    if (wasActive.current && !active) {
      void queryClient.invalidateQueries({ queryKey: qk.runs.trace(runId) });
    }
    wasActive.current = active;
  }, [active, runId, queryClient]);

  useEffect(() => {
    if (!active) return;
    const stream = new GraphEventStream(graphId, {
      onEvent: (event) => {
        if (event.type === 'trace.span' && event.data.span.run_id === runId) {
          const { span } = event.data;
          queryClient.setQueryData<Span[]>(qk.runs.trace(runId), (spans) => mergeSpan(spans, span));
        } else if (event.type === 'run.finished' && event.data.run.id === runId) {
          queryClient.setQueryData(qk.runs.detail(runId), event.data.run);
        }
      },
      onReconnect: () => void queryClient.invalidateQueries({ queryKey: qk.runs.trace(runId) }),
    });
    stream.start();
    return () => stream.stop();
  }, [active, graphId, runId, queryClient]);
}
