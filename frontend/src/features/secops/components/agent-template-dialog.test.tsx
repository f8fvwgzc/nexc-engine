import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { AgentCard } from '@/schemas/agent';
import type { AgentTemplate, CreateAgentFromTemplateBody } from '@/schemas/secops';
import type { SkillSummary } from '@/schemas/skill';
import { agentCard, agentTemplate, ids, skillSummaries } from '@/test/fixtures';

import { AgentTemplateDialog } from './agent-template-dialog';

const { createAgentFromTemplate, catalogue } = vi.hoisted(() => ({
  createAgentFromTemplate:
    vi.fn<(name: string, body: CreateAgentFromTemplateBody) => Promise<AgentCard>>(),
  catalogue: { templates: [] as AgentTemplate[], skills: [] as SkillSummary[] },
}));

vi.mock('../api', () => ({
  createAgentFromTemplate,
  agentTemplatesQuery: () => ({
    queryKey: ['secops', 'templates'],
    queryFn: () => Promise.resolve(catalogue.templates),
  }),
}));
vi.mock('@/features/skills/api', () => ({
  skillsQuery: () => ({
    queryKey: ['skills', 'list'],
    queryFn: () => Promise.resolve(catalogue.skills),
  }),
}));

const analyst: AgentTemplate = {
  ...agentTemplate,
  id: 'analyst',
  name: 'Financial Analyst',
  title: 'Senior Financial Analyst',
  department: 'Finance & Risk',
  credentials: ['CFA® Program Levels I, II and III (CFA Institute)'],
  skills: ['fin-equity-valuation'],
};

const auditor: AgentTemplate = {
  ...agentTemplate,
  id: 'internal-auditor',
  name: 'Internal Auditor',
  title: 'Internal Auditor',
  department: 'Finance & Risk',
  credentials: ['CIA® (The IIA)'],
  skills: [],
};

const sre: AgentTemplate = {
  ...agentTemplate,
  id: 'devops-sre',
  name: 'DevOps / SRE Engineer',
  title: 'Site Reliability Engineer',
  department: 'IT, Cloud & Security',
  credentials: ['CKA (CNCF)'],
};

function renderDialog() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <AgentTemplateDialog workspaceId={ids.workspace} open onOpenChange={() => undefined} />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  createAgentFromTemplate.mockReset();
  // Out of department order, as the catalogue may list them.
  catalogue.templates = [sre, analyst, agentTemplate, auditor];
  catalogue.skills = skillSummaries;
});

describe('AgentTemplateDialog', () => {
  it('lists the templates by department and shows what the chosen one is benchmarked against', async () => {
    renderDialog();
    const radios = await screen.findByRole('radiogroup', { name: 'Template' });
    expect(
      within(radios)
        .getAllByRole('group')
        .map((g) => g.getAttribute('aria-label')),
    ).toEqual(['Core team', 'Finance & Risk', 'IT, Cloud & Security']);
    expect(within(radios).getByRole('radio', { name: /researcher/i })).toBeChecked();
    expect(screen.getByText(/4 roles in 3 departments/)).toBeInTheDocument();

    fireEvent.click(within(radios).getByRole('radio', { name: /financial analyst/i }));
    expect(screen.getByText('Senior Financial Analyst · Finance & Risk')).toBeInTheDocument();
    expect(screen.getByText('Benchmarked against')).toBeInTheDocument();
    expect(
      screen.getAllByText('CFA® Program Levels I, II and III (CFA Institute)').length,
    ).toBeGreaterThan(0);
    expect(screen.getByText(/holds none of these credentials/)).toBeInTheDocument();
    expect(screen.getByText('1 skill · 2 competency questions')).toBeInTheDocument();
    expect(await screen.findByText('Equity valuation')).toBeInTheDocument();
  });

  it('narrows the list with a search over roles, departments and certifications', async () => {
    renderDialog();
    const search = await screen.findByRole('searchbox', { name: 'Search templates' });
    fireEvent.change(search, { target: { value: 'cka' } });
    const radios = screen.getByRole('radiogroup', { name: 'Template' });
    expect(within(radios).getAllByRole('radio')).toHaveLength(1);
    // The first match is the one chosen until another is picked.
    expect(screen.getByRole('button', { name: 'Create DevOps / SRE Engineer' })).toBeEnabled();

    fireEvent.change(search, { target: { value: 'finance' } });
    expect(within(radios).getAllByRole('radio')).toHaveLength(2);

    fireEvent.change(search, { target: { value: 'astronaut' } });
    expect(screen.getByText('No template matches “astronaut”.')).toBeInTheDocument();
  });

  it('makes the agent from the chosen template', async () => {
    createAgentFromTemplate.mockResolvedValue(agentCard);
    renderDialog();
    fireEvent.click(await screen.findByRole('radio', { name: /internal auditor/i }));
    fireEvent.click(screen.getByRole('button', { name: 'Create Internal Auditor' }));
    await waitFor(() =>
      expect(createAgentFromTemplate).toHaveBeenCalledWith('internal-auditor', {
        workspace_id: ids.workspace,
        evaluate: true,
      }),
    );
  });
});
