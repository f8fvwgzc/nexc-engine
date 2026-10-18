import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AUDIT_QUESTIONS, type AuditAnswer } from '@/schemas/audit';

import type { AuditStreamEvent } from '../api';
import { AskAuditPanel } from './ask-audit-panel';

const { askAuditStream } = vi.hoisted(() => ({
  askAuditStream:
    vi.fn<
      (
        workspaceId: string,
        question: string,
        onEvent: (event: AuditStreamEvent) => void,
      ) => Promise<AuditAnswer>
    >(),
}));

vi.mock('../api', () => ({
  askAuditStream,
  auditQuestionsQuery: () => ({
    queryKey: ['audit', 'questions'],
    queryFn: () => Promise.resolve(AUDIT_QUESTIONS),
  }),
}));

const unverified: AuditAnswer = {
  answer: 'I believe the writer leaked the quarterly numbers.',
  mode: 'llm',
  facts: [],
  tokens: 1260,
  latency_ms: 812,
  unverified: true,
  question: null,
};

const deterministic: AuditAnswer = {
  answer: '1 agent read restricted data this week: writer read 1 item.',
  mode: 'deterministic',
  facts: [
    { id: 'agent:w/writer', kind: 'agent', label: 'writer', link: '/app/agents?agent=a1' },
    {
      id: 'withheld:document:1',
      kind: 'withheld',
      label: 'a restricted document (withheld: not in your clearance)',
      link: null,
    },
  ],
  tokens: 0,
  latency_ms: 0.87,
  unverified: false,
  question: 'restricted_reads',
};

function answerWith(answer: AuditAnswer) {
  askAuditStream.mockImplementation((_w, _q, onEvent) => {
    onEvent({ type: 'done', answer });
    return Promise.resolve(answer);
  });
}

function renderPanel(props: Partial<React.ComponentProps<typeof AskAuditPanel>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AskAuditPanel workspaceId="ws-1" {...props} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  askAuditStream.mockReset();
});

describe('AskAuditPanel', () => {
  it('marks an answer that cites no fact as unverified', async () => {
    answerWith(unverified);
    renderPanel();
    fireEvent.click(
      await screen.findByRole('button', { name: 'Which agents read restricted data this week?' }),
    );
    const answer = await screen.findByTestId('audit-answer');
    expect(answer).toHaveAttribute('data-unverified', 'true');
    expect(answer).toHaveAttribute('data-mode', 'llm');
    expect(screen.getByRole('status')).toHaveTextContent(/unverified: this answer cites no fact/i);
    expect(screen.getByText(/llm · 1,260 tokens/)).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Facts' })).not.toBeInTheDocument();
    expect(askAuditStream).toHaveBeenCalledWith(
      'ws-1',
      'Which agents read restricted data this week?',
      expect.any(Function),
      expect.any(AbortSignal),
    );
  });

  it('shows a deterministic answer as free and quick, with its facts as links', async () => {
    answerWith(deterministic);
    renderPanel();
    fireEvent.change(screen.getByLabelText('Question'), {
      target: { value: 'who read restricted data' },
    });
    fireEvent.submit(screen.getByLabelText('Question').closest('form')!);
    const answer = await screen.findByTestId('audit-answer');
    expect(answer).toHaveAttribute('data-unverified', 'false');
    expect(answer).toHaveAttribute('data-mode', 'deterministic');
    expect(screen.getByText(/deterministic · 0 tokens · 0\.87 ms/)).toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    const facts = screen.getByRole('list', { name: 'Facts' });
    expect(facts).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'writer' })).toHaveAttribute(
      'href',
      '/app/agents?agent=a1',
    );
    expect(
      screen.getByText(/a restricted document \(withheld/).closest('[data-withheld]'),
    ).toHaveAttribute('data-withheld', 'true');
  });

  it('fills the agent slot of the chips on an agent card and asks for a run id otherwise', async () => {
    answerWith(deterministic);
    renderPanel({ agentName: 'writer', compact: true });
    fireEvent.click(await screen.findByRole('button', { name: 'What did agent writer do today?' }));
    expect(askAuditStream).toHaveBeenCalledWith(
      'ws-1',
      'What did agent writer do today?',
      expect.any(Function),
      expect.any(AbortSignal),
    );
    await screen.findByTestId('audit-answer');
    fireEvent.click(screen.getByRole('button', { name: 'Why was run … denied?' }));
    expect(screen.getByLabelText('Question')).toHaveValue('Why was run ');
    expect(askAuditStream).toHaveBeenCalledTimes(1);
  });

  it('shows the failure when the audit is off', async () => {
    askAuditStream.mockRejectedValue(
      new Error('Ask the audit is off: no onto-intelli is configured (set NEXC_ONTO_URL)'),
    );
    renderPanel();
    fireEvent.click(await screen.findByRole('button', { name: 'Who approved what this month?' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/NEXC_ONTO_URL/);
  });
});
