import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { A2aTask, Agent } from '@/schemas/agent';
import { agent, ids } from '@/test/fixtures';

import { AgentA2a } from './agent-a2a';

const { setAgentA2aExposed, tasks } = vi.hoisted(() => ({
  setAgentA2aExposed: vi.fn<(agentId: string, exposed: boolean) => Promise<Agent>>(),
  tasks: { list: [] as A2aTask[] },
}));

vi.mock('../api', () => ({
  setAgentA2aExposed,
  agentA2aTasksQuery: (agentId: string) => ({
    queryKey: ['agents', 'a2a-tasks', agentId],
    queryFn: () => Promise.resolve(tasks.list),
  }),
}));

const received: A2aTask = {
  id: '0199aaaa-0000-7000-8000-00000000b001',
  workspace_id: ids.workspace,
  agent_id: ids.agent,
  caller_agent_id: '0199aaaa-0000-7000-8000-00000000b002',
  caller_principal: `agent://${ids.workspace}/a2a:partner`,
  run_id: ids.run,
  node_id: ids.nodeA,
  session_id: null,
  hop: 0,
  chain: [],
  refused_rule: null,
  refused_reason: null,
  title: 'Quarterly report',
  state: 'completed',
  created_at: '2026-10-18T09:00:00Z',
};

const refused: A2aTask = {
  ...received,
  id: '0199aaaa-0000-7000-8000-00000000b003',
  hop: 2,
  chain: ['agent://other/X', 'agent://other/Y'],
  refused_rule: 'delegation.max_depth',
  refused_reason: 'A2A hop 2 exceeds the delegation depth of 1',
  title: 'Too deep',
  state: 'failed',
};

function renderSection(shown: Agent, admin = true) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AgentA2a agent={shown} admin={admin} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  setAgentA2aExposed.mockReset();
  tasks.list = [received, refused];
});

describe('AgentA2a', () => {
  it('publishes an agent from the switch and shows its addresses once it is', async () => {
    setAgentA2aExposed.mockResolvedValue({ ...agent, a2a_exposed: true });
    renderSection(agent);
    expect(screen.queryByText('Card URL')).not.toBeInTheDocument();
    expect(screen.getByText(/publish the agent and the tasks/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('switch', { name: 'Published over A2A' }));
    await waitFor(() => expect(setAgentA2aExposed).toHaveBeenCalledWith(ids.agent, true));
  });

  it('lists the inbound tasks with who called, the hop and the refusal', async () => {
    renderSection({ ...agent, a2a_exposed: true });
    const list = await screen.findByRole('list', { name: 'Inbound tasks' });
    expect(list).toHaveTextContent('Quarterly report');
    expect(list).toHaveTextContent('from a2a:partner');
    expect(list).toHaveTextContent('completed');
    expect(list).toHaveTextContent('hop 2');
    expect(list).toHaveTextContent('refused · delegation.max_depth');
    expect(screen.getAllByRole('link')[0]).toHaveAttribute('href', `/app/runs/${ids.run}`);
    // The addresses carry the principal, percent-encoded, with a copy each.
    expect(screen.getByText('Card URL')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy card url' })).toBeInTheDocument();
    expect(screen.getByText(/\/a2a\/agents\/agent%3A%2F%2F.*\/card$/)).toBeInTheDocument();
  });

  it('keeps the switch from members and from external agents', () => {
    renderSection(agent, false);
    expect(screen.getByRole('switch', { name: 'Published over A2A' })).toBeDisabled();
    expect(screen.getByText(/admins publish and withdraw/i)).toBeInTheDocument();
    renderSection({ ...agent, runtime: 'external' });
    expect(screen.getByText(/cannot take A2A tasks here/i)).toBeInTheDocument();
  });
});
