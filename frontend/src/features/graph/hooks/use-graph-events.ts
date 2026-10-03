import { useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { qk } from '@/lib/query-keys';
import type { ConnectionState } from '@/lib/realtime/connection-state';
import { GraphEventStream } from '@/lib/realtime/sse';
import type { SseEvent } from '@/schemas/realtime';
import type { Artifact, Run } from '@/schemas/run';
import { useGraphStore } from '@/stores/graph-store';

import { fetchPlan } from '../api';
import { createStreamBuffer, type StreamBuffer } from './stream-buffer';

function cacheRun(queryClient: QueryClient, graphId: string, run: Run) {
  queryClient.setQueryData(qk.runs.detail(run.id), run);
  queryClient.setQueryData<Run[]>(qk.graphs.runs(graphId), (runs) =>
    runs ? [run, ...runs.filter((r) => r.id !== run.id)] : runs,
  );
}

function handleEvent(
  event: SseEvent,
  graphId: string,
  queryClient: QueryClient,
  buffer: StreamBuffer,
) {
  const store = useGraphStore.getState();
  switch (event.type) {
    case 'plan.started':
      store.planStarted(event.data.plan_id);
      break;
    case 'plan.node':
      store.planNode(event.data.plan_id, event.data.node);
      break;
    case 'plan.edge':
      store.planEdge(event.data.plan_id, event.data.edge);
      break;
    case 'plan.ready':
      store.planReady(event.data.plan);
      break;
    case 'plan.failed':
      store.planFailed(event.data.plan_id, event.data.error);
      toast.error('Planning failed', { description: event.data.error });
      break;
    case 'run.started':
      store.runUpdated(event.data.run);
      cacheRun(queryClient, graphId, event.data.run);
      break;
    case 'node.status': {
      const { run_id, node_id, status, attempt, cached, error } = event.data;
      store.nodeStatus(run_id, node_id, { status, attempt, cached, error });
      break;
    }
    case 'node.output':
      buffer.output(event.data.run_id, event.data.node_id, event.data.delta);
      break;
    case 'node.log':
      buffer.log(event.data.run_id, event.data.node_id, {
        level: event.data.level,
        message: event.data.message,
        at: Date.now(),
      });
      break;
    case 'node.tokens':
      store.setTokens(event.data.run_id, event.data.node_id, event.data);
      break;
    case 'artifact.created': {
      const { artifact } = event.data;
      queryClient.setQueryData<Artifact[]>(qk.runs.artifacts(artifact.run_id), (list) =>
        list ? [...list.filter((a) => a.id !== artifact.id), artifact] : list,
      );
      toast.success('Artifact ready', { description: artifact.path });
      break;
    }
    case 'run.finished': {
      const { run } = event.data;
      buffer.flush();
      store.runUpdated(run);
      cacheRun(queryClient, graphId, run);
      void queryClient.invalidateQueries({ queryKey: qk.graphs.detail(graphId), exact: true });
      void queryClient.invalidateQueries({ queryKey: qk.runs.artifacts(run.id) });
      if (run.status === 'succeeded') toast.success('Run finished');
      else if (run.status === 'failed') toast.error('Run failed — open the run for details');
      break;
    }
    case 'heartbeat':
      break;
  }
}

/** Subscribes to the graph's SSE stream (plans + runs) and writes into the store / query cache. */
export function useGraphEvents(graphId: string): ConnectionState {
  const queryClient = useQueryClient();
  const [state, setState] = useState<ConnectionState>('connecting');

  useEffect(() => {
    const buffer = createStreamBuffer((runId, pending) => {
      const store = useGraphStore.getState();
      store.appendOutputs(runId, pending.outputs);
      store.appendLogs(runId, pending.logs);
    });

    const resync = async () => {
      void queryClient.invalidateQueries({ queryKey: qk.graphs.detail(graphId), exact: true });
      void queryClient.invalidateQueries({ queryKey: qk.graphs.runs(graphId) });
      const plan = useGraphStore.getState().plan;
      if (plan?.id && plan.status === 'streaming') {
        try {
          useGraphStore.getState().planReady(await fetchPlan(graphId, plan.id));
        } catch {
          // The next plan.* event or a manual retry will recover.
        }
      }
    };

    const stream = new GraphEventStream(graphId, {
      onEvent: (event) => handleEvent(event, graphId, queryClient, buffer),
      onStateChange: setState,
      onReconnect: () => void resync(),
    });
    stream.start();
    return () => {
      stream.stop();
      buffer.cancel();
    };
  }, [graphId, queryClient]);

  return state;
}
