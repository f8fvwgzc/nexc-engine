import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiError } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Agent, AgentCard, AgentEvaluation } from '@/schemas/agent';
import type { AgentTemplate } from '@/schemas/secops';
import { agent, agentCard, agentTemplate, ids } from '@/test/fixtures';

import { AgentCompetency } from './agent-competency';

const { evaluateAgent, catalogue } = vi.hoisted(() => ({
  evaluateAgent: vi.fn<(agentId: string) => Promise<AgentEvaluation>>(),
  catalogue: { templates: [] as AgentTemplate[] },
}));

vi.mock('../api', () => ({ evaluateAgent }));
vi.mock('@/features/secops/api', () => ({
  agentTemplatesQuery: () => ({
    queryKey: ['secops', 'templates'],
    queryFn: () => Promise.resolve(catalogue.templates),
  }),
}));
vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useWorkspaceId: () => ids.workspace,
}));

const analyst: AgentTemplate = {
  ...agentTemplate,
  id: 'analyst',
  name: 'Financial Analyst',
  department: 'Finance & Risk',
  credentials: ['CFA® Program Levels I, II and III (CFA Institute)', 'GIPS® standards'],
  questions: Array.from({ length: 5 }, (_, i) => ({
    question: `Question ${i + 1}?`,
    expected: ['answer'],
  })),
};

const scored: AgentEvaluation = {
  template: 'analyst',
  score: 4,
  total: 5,
  model: 'claude-opus-5',
  questions: analyst.questions.map((q, i) => ({ ...q, passed: i > 0, answer: `Answer ${i}` })),
  evaluated_at: '2026-10-24T09:00:00Z',
};

const shown: Agent = { ...agent, template_id: 'analyst' };

function renderSection(props: { agent?: Agent; member?: boolean } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  client.setQueryData<AgentCard>(qk.agents.card(ids.agent), { ...agentCard, agent: shown });
  render(
    <QueryClientProvider client={client}>
      <AgentCompetency
        agent={props.agent ?? shown}
        evaluation={null}
        member={props.member ?? true}
      />
    </QueryClientProvider>,
  );
  return client;
}

beforeEach(() => {
  evaluateAgent.mockReset();
  catalogue.templates = [agentTemplate, analyst];
});

describe('AgentCompetency', () => {
  it('lists what the template is benchmarked against, and says the agent holds none of it', async () => {
    renderSection();
    const section = await screen.findByRole('region', { name: 'Benchmarked against' });
    expect(section).toHaveTextContent('CFA® Program Levels I, II and III (CFA Institute)');
    expect(section).toHaveTextContent('GIPS® standards');
    expect(section).toHaveTextContent(/holds none of these credentials/);
    expect(screen.queryByText(/certified/i)).not.toBeInTheDocument();
    expect(screen.getByText(/the check asks its template’s 5 questions/)).toBeInTheDocument();
  });

  it('runs the competency check, says it is asking, and keeps the score on the card', async () => {
    let resolve: (value: AgentEvaluation) => void = () => undefined;
    evaluateAgent.mockReturnValue(new Promise((r) => (resolve = r)));
    const client = renderSection();
    fireEvent.click(await screen.findByRole('button', { name: /run competency check/i }));
    await waitFor(() => expect(evaluateAgent).toHaveBeenCalledWith(ids.agent));
    expect(await screen.findByText(/Asking its 5 questions again/)).toHaveAttribute(
      'role',
      'status',
    );
    expect(screen.getByRole('button', { name: /asking/i })).toBeDisabled();
    resolve(scored);
    await waitFor(() =>
      expect(client.getQueryData<AgentCard>(qk.agents.card(ids.agent))?.evaluation).toEqual(scored),
    );
    expect(client.getQueryState(qk.agents.card(ids.agent))?.isInvalidated).toBe(true);
  });

  it('shows why the server refused the check', async () => {
    evaluateAgent.mockRejectedValue(
      new ApiError({
        type: 'about:blank',
        title: 'Unprocessable Entity',
        status: 422,
        detail: 'Validation failed',
        errors: {
          provider: ["no usable model provider answered: check the workspace's model settings"],
        },
      }),
    );
    renderSection();
    fireEvent.click(await screen.findByRole('button', { name: /run competency check/i }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "No usable model provider answered: check the workspace's model settings",
    );
  });

  it('keeps the check from guests, and shows nothing for an agent made by hand', async () => {
    renderSection({ member: false });
    await screen.findByRole('region', { name: 'Benchmarked against' });
    expect(screen.queryByRole('button', { name: /run competency check/i })).toBeNull();
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <AgentCompetency agent={{ ...agent, template_id: null }} evaluation={null} member />
      </QueryClientProvider>,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
