import { create } from 'zustand';

import {
  createContextSlice,
  createPlanSlice,
  createPresenceSlice,
  createRunSlice,
  createSelectionSlice,
  createViewportSlice,
} from './graph/slices';
import type { GraphCanvasState } from './graph/types';

export type * from './graph/types';

/** UI + live realtime state of the graph canvas. Server state stays in React Query. */
export const useGraphStore = create<GraphCanvasState>()((...a) => ({
  ...createSelectionSlice(...a),
  ...createViewportSlice(...a),
  ...createPlanSlice(...a),
  ...createRunSlice(...a),
  ...createPresenceSlice(...a),
  ...createContextSlice(...a),
}));

const TERMINAL = new Set<string>(['succeeded', 'failed', 'skipped', 'cancelled']);

export interface RunProgress {
  done: number;
  total: number;
  running: number;
  failed: number;
  tokensIn: number;
  tokensOut: number;
}

/** Derived progress for the active run. Use with `useShallow` — it returns a fresh object. */
export function selectRunProgress(s: GraphCanvasState): RunProgress | null {
  if (!s.run) return null;
  const states = Object.values(s.nodeStates);
  let tokensIn = 0;
  let tokensOut = 0;
  for (const usage of Object.values(s.tokens)) {
    tokensIn += usage.tokens_in;
    tokensOut += usage.tokens_out;
  }
  return {
    total: Math.max(s.run.node_runs.length, states.length),
    done: states.filter((st) => TERMINAL.has(st.status)).length,
    running: states.filter((st) => st.status === 'running').length,
    failed: states.filter((st) => st.status === 'failed').length,
    tokensIn: Math.max(tokensIn, s.run.tokens_in),
    tokensOut: Math.max(tokensOut, s.run.tokens_out),
  };
}
