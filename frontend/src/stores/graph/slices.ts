import type { StateCreator } from 'zustand';

import type { Run } from '@/schemas/run';

import type {
  ContextSlice,
  GraphCanvasState,
  LiveNodeState,
  PlanSlice,
  PresenceSlice,
  RunSlice,
  SelectionSlice,
  ViewportSlice,
} from './types';

type Slice<T> = StateCreator<GraphCanvasState, [], [], T>;

const MAX_LOG_LINES = 500;

export const createSelectionSlice: Slice<SelectionSlice> = (set) => ({
  selectedNodeId: null,
  selectedEdgeId: null,
  selectNode: (selectedNodeId) => set({ selectedNodeId, selectedEdgeId: null }),
  selectEdge: (selectedEdgeId) => set({ selectedEdgeId, selectedNodeId: null }),
});

export const createViewportSlice: Slice<ViewportSlice> = (set) => ({
  transforms: {},
  setTransform: (graphId, transform) =>
    set((s) => ({ transforms: { ...s.transforms, [graphId]: transform } })),
});

const emptyPlan = { summary: '', nodes: [], edges: [], error: null };

export const createPlanSlice: Slice<PlanSlice> = (set) => ({
  plan: null,
  planRequested: () => set({ plan: { id: null, status: 'requesting', ...emptyPlan } }),
  planStarted: (planId) =>
    set((s) =>
      s.plan?.id === planId ? s : { plan: { id: planId, status: 'streaming', ...emptyPlan } },
    ),
  planNode: (planId, node) =>
    set((s) => {
      const plan = adoptPlan(s.plan, planId);
      if (!plan) return s;
      const nodes = plan.nodes.some((n) => n.ref === node.ref)
        ? plan.nodes.map((n) => (n.ref === node.ref ? node : n))
        : [...plan.nodes, node];
      return { plan: { ...plan, nodes } };
    }),
  planEdge: (planId, edge) =>
    set((s) => {
      const plan = adoptPlan(s.plan, planId);
      if (!plan) return s;
      const exists = plan.edges.some(
        (e) => e.source_ref === edge.source_ref && e.target_ref === edge.target_ref,
      );
      return exists ? s : { plan: { ...plan, edges: [...plan.edges, edge] } };
    }),
  planReady: (plan) =>
    set((s) =>
      s.plan && s.plan.id !== null && s.plan.id !== plan.id && s.plan.status === 'streaming'
        ? s
        : { plan: { ...plan } },
    ),
  planFailed: (planId, error) =>
    set((s) => {
      const plan = adoptPlan(s.plan, planId);
      return plan ? { plan: { ...plan, status: 'failed', error } } : s;
    }),
  clearPlan: () => set({ plan: null }),
});

/** Events for the active plan (or the first plan while a request is in flight) are accepted. */
function adoptPlan(plan: PlanSlice['plan'], planId: string): PlanSlice['plan'] {
  if (!plan) return { id: planId, status: 'streaming', ...emptyPlan };
  if (plan.id === planId) return plan;
  if (plan.id === null) return { ...plan, id: planId, status: 'streaming' };
  return null;
}

function statesFromRun(run: Run): Record<string, LiveNodeState> {
  return Object.fromEntries(
    run.node_runs.map((nr) => [
      nr.node_id,
      { status: nr.status, attempt: nr.attempt, cached: nr.cached, error: nr.error },
    ]),
  );
}

function tokensFromRun(run: Run): RunSlice['tokens'] {
  return Object.fromEntries(
    run.node_runs.map((nr) => [nr.node_id, { tokens_in: nr.tokens_in, tokens_out: nr.tokens_out }]),
  );
}

export const createRunSlice: Slice<RunSlice> = (set) => ({
  run: null,
  nodeStates: {},
  outputs: {},
  logs: {},
  tokens: {},
  runUpdated: (run) =>
    set((s) => {
      if (s.run && s.run.id === run.id) {
        return {
          run,
          nodeStates: { ...s.nodeStates, ...statesFromRun(run) },
          tokens: { ...s.tokens, ...tokensFromRun(run) },
        };
      }
      if (s.run && Date.parse(s.run.created_at) > Date.parse(run.created_at)) return s;
      return {
        run,
        nodeStates: statesFromRun(run),
        tokens: tokensFromRun(run),
        outputs: {},
        logs: {},
      };
    }),
  nodeStatus: (runId, nodeId, state) =>
    set((s) => (s.run?.id === runId ? { nodeStates: { ...s.nodeStates, [nodeId]: state } } : s)),
  appendOutputs: (runId, deltas) =>
    set((s) => {
      if (s.run?.id !== runId) return s;
      const outputs = { ...s.outputs };
      for (const [nodeId, delta] of Object.entries(deltas)) {
        outputs[nodeId] = (outputs[nodeId] ?? '') + delta;
      }
      return { outputs };
    }),
  appendLogs: (runId, lines) =>
    set((s) => {
      if (s.run?.id !== runId) return s;
      const logs = { ...s.logs };
      for (const [nodeId, batch] of Object.entries(lines)) {
        logs[nodeId] = [...(logs[nodeId] ?? []), ...batch].slice(-MAX_LOG_LINES);
      }
      return { logs };
    }),
  // node.tokens carries the node's cumulative usage (see README "contract notes"): keep the max.
  setTokens: (runId, nodeId, usage) =>
    set((s) => {
      if (s.run?.id !== runId) return s;
      const prev = s.tokens[nodeId];
      const next = {
        tokens_in: Math.max(prev?.tokens_in ?? 0, usage.tokens_in),
        tokens_out: Math.max(prev?.tokens_out ?? 0, usage.tokens_out),
      };
      return { tokens: { ...s.tokens, [nodeId]: next } };
    }),
});

export const createPresenceSlice: Slice<PresenceSlice> = (set) => ({
  peers: {},
  upsertPeer: (userId, name, cursor) =>
    set((s) => ({ peers: { ...s.peers, [userId]: { name, cursor, seenAt: Date.now() } } })),
  prunePeers: (olderThanMs) =>
    set((s) => {
      const cutoff = Date.now() - olderThanMs;
      const entries = Object.entries(s.peers).filter(([, p]) => p.seenAt >= cutoff);
      return entries.length === Object.keys(s.peers).length
        ? s
        : { peers: Object.fromEntries(entries) };
    }),
});

export const createContextSlice: Slice<ContextSlice> = (set, get) => ({
  graphId: null,
  enterGraph: (graphId) => {
    if (get().graphId === graphId) return;
    set({
      graphId,
      selectedNodeId: null,
      selectedEdgeId: null,
      plan: null,
      run: null,
      nodeStates: {},
      outputs: {},
      logs: {},
      tokens: {},
      peers: {},
    });
  },
});
