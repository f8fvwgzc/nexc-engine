import { describe, expect, it } from 'vitest';

import { agent } from '@/test/fixtures';
import type { Agent } from '@/schemas/agent';

import { localToIso, spanHref } from './explorer';
import { filterAgents } from './registry';

const owner = '0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b';
const agents: Agent[] = [
  { ...agent, id: 'a', name: 'Analyst', role: 'analyst', owner_user_id: owner },
  { ...agent, id: 'b', name: 'Scout', role: 'scout', status: 'quarantined', owner_user_id: null },
  {
    ...agent,
    id: 'c',
    name: 'Writer',
    role: 'writer',
    title: 'Lead Writer',
    model: 'claude-x',
    owner_user_id: null,
  },
];

describe('filterAgents', () => {
  const owners = new Map([[owner, 'Ada Lovelace']]);

  it('matches name, role, title, owner and model, case-insensitively', () => {
    expect(filterAgents(agents, 'ana', 'all', owners).map((a) => a.id)).toEqual(['a']);
    expect(filterAgents(agents, 'ada', 'all', owners).map((a) => a.id)).toEqual(['a']);
    expect(filterAgents(agents, 'LEAD', 'all', owners).map((a) => a.id)).toEqual(['c']);
    expect(filterAgents(agents, 'claude-x', 'all', owners).map((a) => a.id)).toEqual(['c']);
    expect(filterAgents(agents, '', 'all', owners)).toHaveLength(3);
  });

  it('narrows by status', () => {
    expect(filterAgents(agents, '', 'quarantined', owners).map((a) => a.id)).toEqual(['b']);
    expect(filterAgents(agents, 'scout', 'active', owners)).toEqual([]);
  });
});

describe('explorer helpers', () => {
  it('reads a local date-time and links a span to its run', () => {
    expect(localToIso('')).toBeUndefined();
    expect(localToIso('not a date')).toBeUndefined();
    expect(localToIso('2026-10-18T09:30')).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/);
    expect(
      spanHref({
        id: 's1',
        run_id: 'r1',
        graph_id: 'g',
        node_id: 'n',
        agent_id: null,
        principal: 'system',
        kind: 'policy',
        name: 'context',
        started_at: '2026-10-18T09:30:00Z',
        ended_at: null,
        status: 'ok',
        tool: null,
        host: null,
        decision: 'deny',
        rule: 'labels',
        model: null,
        tokens_in: null,
        tokens_out: null,
      }),
    ).toBe('/app/runs/r1?span=s1');
  });
});
