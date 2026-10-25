import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import type { Skill, SkillSummary } from '@/schemas/skill';
import { skill, skillSummaries } from '@/test/fixtures';

import { AgentSkills } from './agent-skills';

const { library } = vi.hoisted(() => ({
  library: { list: [] as SkillSummary[], one: null as Skill | null },
}));

vi.mock('@/features/skills/api', () => ({
  skillsQuery: () => ({
    queryKey: ['skills', 'list'],
    queryFn: () => Promise.resolve(library.list),
  }),
  skillQuery: (id: string) => ({
    queryKey: ['skills', 'detail', id],
    queryFn: () => Promise.resolve(library.one),
  }),
}));

function renderSkills(skillIds: string[]) {
  library.list = skillSummaries;
  library.one = skill;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AgentSkills skillIds={skillIds} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('AgentSkills', () => {
  it('names each skill from the library, in the agent’s order', async () => {
    renderSkills(['research-source-evaluation', 'core-evidence-and-sources', 'gone-skill']);
    const list = screen.getByRole('list', { name: 'Skills' });
    expect(await within(list).findByRole('button', { name: 'Source evaluation' })).toBeVisible();
    expect(
      within(list)
        .getAllByRole('button')
        .map((b) => b.textContent),
    ).toEqual([
      'Source evaluation',
      'Evidence and sources',
      // A skill the library no longer has keeps its id.
      'gone-skill',
    ]);
    expect(screen.getByRole('heading', { name: /skills · 3 of 12/i })).toBeInTheDocument();
  });

  it('opens a skill’s working knowledge and its sources, which open in a new tab', async () => {
    renderSkills(['research-source-evaluation']);
    fireEvent.click(await screen.findByRole('button', { name: 'Source evaluation' }));
    const dialog = await screen.findByRole('dialog', { name: 'Source evaluation' });
    expect(await within(dialog).findByRole('heading', { name: 'Checks' })).toBeInTheDocument();
    expect(within(dialog).getByText('when').tagName).toBe('STRONG');
    const source = within(dialog).getByRole('link', { name: /evaluating sources/i });
    expect(source).toHaveAttribute('href', 'https://example.com/evaluating');
    expect(source).toHaveAttribute('target', '_blank');
    expect(source).toHaveAttribute('rel', 'noopener noreferrer');
    expect(
      within(dialog).getByRole('link', { name: /open in the skill library/i }),
    ).toHaveAttribute('href', '/app/skills?skill=research-source-evaluation');
  });

  it('says when the agent has no skills', () => {
    renderSkills([]);
    expect(screen.getByText(/no skills yet/i)).toBeInTheDocument();
  });
});
