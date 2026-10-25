import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { Agent } from '@/schemas/agent';
import type { AgentTemplate } from '@/schemas/secops';
import { agent, agentTemplate } from '@/test/fixtures';

import { AgentOrgChart } from './agent-org-chart';

const { catalogue } = vi.hoisted(() => ({ catalogue: { templates: [] as AgentTemplate[] } }));

vi.mock('../api', () => ({
  agentActivityQuery: (agentId: string) => ({
    queryKey: ['agents', 'activity', agentId],
    queryFn: () => Promise.resolve([]),
  }),
}));
vi.mock('@/features/secops/api', () => ({
  agentTemplatesQuery: () => ({
    queryKey: ['secops', 'templates'],
    queryFn: () => Promise.resolve(catalogue.templates),
  }),
}));

const DEPARTMENTS = ['Finance & Risk', 'IT, Cloud & Security', 'Marketing & Sales'];

const id = (n: number) => `0199aaaa-0000-7000-8000-${String(n).padStart(12, '0')}`;

const planner: Agent = { ...agent, id: id(0), name: 'Planner', role: 'planner', template_id: null };

/** The Planner over four core agents and ten experts in each of three departments. */
function workspaceAgents(): Agent[] {
  const core = ['Researcher', 'Writer', 'Engineer', 'Reviewer'].map((name, i) => ({
    ...agent,
    id: id(i + 1),
    name,
    role: name.toLowerCase(),
    template_id: null,
    reports_to: planner.id,
  }));
  const experts = DEPARTMENTS.flatMap((_, d) =>
    Array.from({ length: 10 }, (_, i) => ({
      ...agent,
      id: id(100 + d * 10 + i),
      name: `Expert ${d}-${i}`,
      role: `expert_${d}_${i}`,
      template_id: `t${d}`,
      reports_to: planner.id,
    })),
  );
  return [planner, ...core, ...experts];
}

function renderChart(agents: Agent[], onEdit = vi.fn<(agent: Agent) => void>()) {
  catalogue.templates = DEPARTMENTS.map((department, d) => ({
    ...agentTemplate,
    id: `t${d}`,
    department,
  }));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AgentOrgChart agents={agents} onEdit={onEdit} />
    </QueryClientProvider>,
  );
  return onEdit;
}

describe('AgentOrgChart', () => {
  it('shows a long list of direct reports by department, a column each', async () => {
    renderChart(workspaceAgents());
    const reports = screen.getByRole('list', { name: 'Reports to Planner, by department' });
    // Each heading says the department and how many of its agents report here.
    await waitFor(() =>
      expect(
        within(reports)
          .getAllByRole('heading', { level: 3 })
          .map((h) => h.textContent),
      ).toEqual([
        'Core team4',
        'Finance & Risk10',
        'IT, Cloud & Security10',
        'Marketing & Sales10',
      ]),
    );
    expect(within(reports).getAllByRole('region')).toHaveLength(4);
    // Only the Planner keeps a full card (and its activity); the reports are compact rows.
    expect(await screen.findAllByText('No activity yet')).toHaveLength(1);
  });

  it('caps a long column with a way to show the rest', async () => {
    const onEdit = renderChart(workspaceAgents());
    const finance = await screen.findByRole('region', { name: /finance & risk/i });
    expect(within(finance).getAllByRole('button', { name: /^Edit / })).toHaveLength(6);
    fireEvent.click(within(finance).getByRole('button', { name: 'Show 4 more' }));
    expect(within(finance).getAllByRole('button', { name: /^Edit / })).toHaveLength(10);
    fireEvent.click(within(finance).getByRole('button', { name: 'Edit Expert 0-9' }));
    expect(onEdit).toHaveBeenCalledWith(expect.objectContaining({ name: 'Expert 0-9' }));
  });

  it('keeps one card per report for a small team', () => {
    renderChart(workspaceAgents().slice(0, 5));
    expect(screen.queryByRole('list', { name: /by department/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Edit Writer' })).toBeInTheDocument();
    expect(screen.queryAllByRole('region')).toHaveLength(0);
  });
});
