import { beforeEach, describe, expect, it } from 'vitest';

import * as f from '@/test/fixtures';

import { selectRunProgress, useGraphStore } from './graph-store';

const { run } = f;

beforeEach(() => {
  useGraphStore.setState({ graphId: null });
  useGraphStore.getState().enterGraph(f.ids.graph);
});

describe('graph store — plan slice', () => {
  it('streams ghost nodes for the active plan and ignores other plans', () => {
    const s = useGraphStore.getState();
    s.planRequested();
    s.planNode(f.ids.plan, f.plan.nodes[0]!);
    s.planNode(f.ids.run, { ...f.plan.nodes[0]!, ref: 'other' });
    s.planEdge(f.ids.plan, { source_ref: 'n1', target_ref: 'n2' });
    s.planEdge(f.ids.plan, { source_ref: 'n1', target_ref: 'n2' });
    const plan = useGraphStore.getState().plan!;
    expect(plan.id).toBe(f.ids.plan);
    expect(plan.status).toBe('streaming');
    expect(plan.nodes.map((n) => n.ref)).toEqual(['n1']);
    expect(plan.edges).toHaveLength(1);
  });
});

describe('graph store — run slice', () => {
  it('hydrates node states from a run and derives progress', () => {
    const s = useGraphStore.getState();
    s.runUpdated(run);
    s.nodeStatus(run.id, f.ids.nodeB, {
      status: 'running',
      attempt: 1,
      cached: false,
      error: null,
    });
    s.setTokens(run.id, f.ids.nodeB, { tokens_in: 50, tokens_out: 10 });
    s.appendOutputs(run.id, { [f.ids.nodeB]: 'Hello' });
    s.appendOutputs(run.id, { [f.ids.nodeB]: ', world' });
    const state = useGraphStore.getState();
    expect(state.outputs[f.ids.nodeB]).toBe('Hello, world');
    expect(selectRunProgress(state)).toEqual({
      total: 2,
      done: 1,
      running: 1,
      failed: 0,
      tokensIn: 1250,
      tokensOut: 350,
    });
  });

  it('drops events for runs other than the active one', () => {
    const s = useGraphStore.getState();
    s.runUpdated(run);
    s.appendOutputs(f.ids.plan, { [f.ids.nodeA]: 'stray' });
    expect(useGraphStore.getState().outputs).toEqual({});
  });
});
