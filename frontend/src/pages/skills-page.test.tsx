import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { HelmetProvider } from 'react-helmet-async';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Agent } from '@/schemas/agent';
import type { Skill, SkillSummary } from '@/schemas/skill';
import { agent, ids, skill, skillSummaries } from '@/test/fixtures';

import SkillsPage from './skills-page';

const { library } = vi.hoisted(() => ({
  library: { list: [] as SkillSummary[], skills: [] as Skill[], agents: [] as Agent[] },
}));

vi.mock('@/features/skills/api', () => ({
  skillsQuery: () => ({
    queryKey: ['skills', 'list'],
    queryFn: () => Promise.resolve(library.list),
  }),
  skillQuery: (id: string) => ({
    queryKey: ['skills', 'detail', id],
    queryFn: () => Promise.resolve(library.skills.find((s) => s.id === id)),
  }),
}));
vi.mock('@/features/agents/api', () => ({
  agentsQuery: (workspaceId: string) => ({
    queryKey: ['agents', workspaceId],
    queryFn: () => Promise.resolve(library.agents),
  }),
}));
vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useWorkspaceId: () => ids.workspace,
}));

function renderPage(path = '/app/skills') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <HelmetProvider>
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={[path]}>
          <Routes>
            <Route path="/app/skills" element={<SkillsPage />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </HelmetProvider>,
  );
}

beforeEach(() => {
  library.list = skillSummaries;
  library.skills = skillSummaries.map(({ chars: _chars, ...summary }) => ({
    ...summary,
    body: summary.id === skill.id ? skill.body : `About ${summary.name}.`,
  }));
  library.agents = [agent];
});

describe('SkillsPage', () => {
  it('lists the skills by category and shows the first one', async () => {
    renderPage();
    const list = await screen.findByRole('group', { name: /^core/i });
    expect(within(list).getByRole('button', { name: 'Evidence and sources' })).toHaveAttribute(
      'aria-current',
      'true',
    );
    expect(screen.getByRole('group', { name: /^finance/i })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 2, name: 'Evidence and sources' })).toBeVisible();
    expect(await screen.findByText('About Evidence and sources.')).toBeInTheDocument();
    expect(screen.getByText(/3 skills in 3 fields/)).toBeInTheDocument();
  });

  it('opens the skill a link names, with its knowledge, sources and the agents that have it', async () => {
    renderPage('/app/skills?skill=research-source-evaluation');
    expect(
      await screen.findByRole('heading', { level: 2, name: 'Source evaluation' }),
    ).toBeVisible();
    expect(await screen.findByRole('heading', { name: 'Checks' })).toBeInTheDocument();
    const source = screen.getByRole('link', { name: /evaluating sources/i });
    expect(source).toHaveAttribute('target', '_blank');
    expect(source).toHaveAttribute('rel', 'noopener noreferrer');
    expect(await screen.findByRole('link', { name: 'Ada' })).toHaveAttribute(
      'href',
      `/app/agents?agent=${ids.agent}`,
    );
  });

  it('narrows the list with a search and shows the skill picked', async () => {
    renderPage();
    fireEvent.change(await screen.findByRole('searchbox', { name: 'Search skills' }), {
      target: { value: 'dcf' },
    });
    expect(screen.queryByRole('group', { name: /^core/i })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Equity valuation' }));
    expect(
      await screen.findByRole('heading', { level: 2, name: 'Equity valuation' }),
    ).toBeVisible();
    expect(screen.getByText('No agent of this workspace has it yet.')).toBeInTheDocument();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search skills' }), {
      target: { value: 'astronaut' },
    });
    expect(screen.getByText('No skill matches “astronaut”.')).toBeInTheDocument();
  });

  it('says so when a link names a skill the library does not have', async () => {
    renderPage('/app/skills?skill=gone');
    expect(await screen.findByText('No such skill')).toBeInTheDocument();
  });
});
