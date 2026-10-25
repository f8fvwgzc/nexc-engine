import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Agent, AgentInput } from '@/schemas/agent';
import type { SkillSummary } from '@/schemas/skill';
import { agent, ids, skillSummaries } from '@/test/fixtures';

import { AgentFormDialog } from './agent-form-dialog';

const { updateAgent, createAgent, library } = vi.hoisted(() => ({
  updateAgent: vi.fn<(agentId: string, body: AgentInput) => Promise<Agent>>(),
  createAgent: vi.fn<(body: AgentInput) => Promise<Agent>>(),
  library: { list: [] as SkillSummary[] },
}));

vi.mock('../api', () => ({
  updateAgent,
  createAgent,
  deleteAgent: vi.fn(),
  quarantineAgent: vi.fn(),
  releaseAgent: vi.fn(),
}));
vi.mock('@/features/skills/api', () => ({
  skillsQuery: () => ({
    queryKey: ['skills', 'list'],
    queryFn: () => Promise.resolve(library.list),
  }),
}));
vi.mock('@/features/workspaces/api', () => ({
  teamsQuery: (workspaceId: string) => ({
    queryKey: ['workspaces', workspaceId, 'teams'],
    queryFn: () => Promise.resolve([]),
  }),
}));
vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useWorkspaceId: () => ids.workspace,
}));

function renderForm(editing: Agent | null) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AgentFormDialog
        open
        onOpenChange={() => undefined}
        agent={editing}
        agents={editing ? [editing] : []}
      />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  updateAgent.mockReset();
  createAgent.mockReset();
  library.list = skillSummaries;
});

describe('AgentFormDialog skills', () => {
  it('shows the agent’s skills and saves the list as changed', async () => {
    updateAgent.mockImplementation((_id, body) => Promise.resolve({ ...agent, ...body }));
    renderForm(agent);
    expect(
      await screen.findByRole('button', { name: 'Remove Source evaluation' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add skills, 2 of 12 chosen' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove Source evaluation' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(updateAgent).toHaveBeenCalled());
    expect(updateAgent.mock.calls[0]?.[0]).toBe(ids.agent);
    expect(updateAgent.mock.calls[0]?.[1].skills).toEqual(['core-evidence-and-sources']);
  });

  it('starts a new agent with no skills', async () => {
    renderForm(null);
    expect(
      await screen.findByRole('button', { name: 'Add skills, 0 of 12 chosen' }),
    ).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Chosen skills' })).toBeNull();
  });
});
