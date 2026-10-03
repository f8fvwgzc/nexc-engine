import type { NodeStatus } from '@/schemas/graph';
import type { PlanStatus, ProposedEdge, ProposedNode } from '@/schemas/plan';
import type { Cursor } from '@/schemas/realtime';
import type { Run } from '@/schemas/run';

export interface ViewTransform {
  x: number;
  y: number;
  k: number;
}

export interface LiveNodeState {
  status: NodeStatus;
  attempt: number;
  cached: boolean;
  error: string | null;
}

export interface LogLine {
  level: string;
  message: string;
  at: number;
}

export interface TokenUsage {
  tokens_in: number;
  tokens_out: number;
}

export interface Peer {
  name: string;
  cursor: Cursor;
  seenAt: number;
}

export interface SelectionSlice {
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  selectNode: (nodeId: string | null) => void;
  selectEdge: (edgeId: string | null) => void;
}

export interface ViewportSlice {
  /** Last zoom transform per graph so returning to a canvas restores the view. */
  transforms: Record<string, ViewTransform>;
  setTransform: (graphId: string, transform: ViewTransform) => void;
}

export interface PlanState {
  id: string | null;
  status: PlanStatus | 'requesting';
  summary: string;
  nodes: ProposedNode[];
  edges: ProposedEdge[];
  error: string | null;
}

export interface PlanSlice {
  plan: PlanState | null;
  planRequested: () => void;
  planStarted: (planId: string) => void;
  planNode: (planId: string, node: ProposedNode) => void;
  planEdge: (planId: string, edge: ProposedEdge) => void;
  planReady: (plan: {
    id: string;
    status: PlanStatus;
    summary: string;
    nodes: ProposedNode[];
    edges: ProposedEdge[];
    error: string | null;
  }) => void;
  planFailed: (planId: string, error: string) => void;
  clearPlan: () => void;
}

export interface RunSlice {
  run: Run | null;
  nodeStates: Record<string, LiveNodeState>;
  outputs: Record<string, string>;
  logs: Record<string, LogLine[]>;
  tokens: Record<string, TokenUsage>;
  /** Run created/started/finished — hydrates node states from `node_runs`. */
  runUpdated: (run: Run) => void;
  nodeStatus: (runId: string, nodeId: string, state: LiveNodeState) => void;
  /** Batched streamed output (node id → concatenated deltas). */
  appendOutputs: (runId: string, deltas: Record<string, string>) => void;
  /** Batched log lines (node id → new lines). */
  appendLogs: (runId: string, lines: Record<string, LogLine[]>) => void;
  setTokens: (runId: string, nodeId: string, usage: TokenUsage) => void;
}

export interface PresenceSlice {
  peers: Record<string, Peer>;
  upsertPeer: (userId: string, name: string, cursor: Cursor) => void;
  prunePeers: (olderThanMs: number) => void;
}

export interface ContextSlice {
  graphId: string | null;
  /** Resets all per-graph state when the canvas switches graphs. */
  enterGraph: (graphId: string) => void;
}

export type GraphCanvasState = SelectionSlice &
  ViewportSlice &
  PlanSlice &
  RunSlice &
  PresenceSlice &
  ContextSlice;
