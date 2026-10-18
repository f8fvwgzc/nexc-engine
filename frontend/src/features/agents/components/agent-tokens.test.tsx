import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { AgentApiToken, IssuedAgentApiToken } from '@/schemas/agent';
import { ids } from '@/test/fixtures';

import { AgentTokens } from './agent-tokens';

const { issueAgentToken, revokeAgentToken, tokens } = vi.hoisted(() => ({
  issueAgentToken: vi.fn<(agentId: string, name: string) => Promise<IssuedAgentApiToken>>(),
  revokeAgentToken: vi.fn<(agentId: string, tokenId: string) => Promise<void>>(),
  tokens: { list: [] as AgentApiToken[] },
}));

vi.mock('../api', () => ({
  issueAgentToken,
  revokeAgentToken,
  agentTokensQuery: (agentId: string) => ({
    queryKey: ['agents', 'tokens', agentId],
    queryFn: () => Promise.resolve(tokens.list),
  }),
}));

const SECRET = 'nexc_agt_0123456789abcdef0123456789abcdef';

const sidecar: AgentApiToken = {
  id: '0199aaaa-0000-7000-8000-00000000a001',
  agent_id: ids.agent,
  prefix: 'nexc_agt_0123',
  name: 'sidecar',
  created_by: ids.user,
  created_at: '2026-10-01T09:00:00Z',
  last_used_at: '2026-10-02T09:00:00Z',
  revoked_at: null,
};

const old: AgentApiToken = {
  ...sidecar,
  id: '0199aaaa-0000-7000-8000-00000000a002',
  prefix: 'nexc_agt_9876',
  name: 'old laptop',
  last_used_at: null,
  revoked_at: '2026-10-03T09:00:00Z',
};

function renderTokens(admin = true) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <AgentTokens agentId={ids.agent} admin={admin} />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  issueAgentToken.mockReset();
  revokeAgentToken.mockReset();
  tokens.list = [sidecar, old];
});

describe('AgentTokens', () => {
  it('lists what is kept of each token: name, prefix, dates, revoked, and never a value', async () => {
    renderTokens();
    const list = await screen.findByRole('list', { name: 'API tokens' });
    expect(list).toHaveTextContent('sidecar');
    expect(list).toHaveTextContent('nexc_agt_0123…');
    expect(list).toHaveTextContent('old laptop');
    expect(list).toHaveTextContent(/revoked/);
    expect(list).toHaveTextContent(/last used never/);
    // One Revoke for the live token; the revoked one offers none.
    expect(screen.getAllByRole('button', { name: /revoke/i })).toHaveLength(1);
    expect(screen.queryByText(SECRET)).not.toBeInTheDocument();
  });

  it('shows an issued token once, with a copy, then only its prefix', async () => {
    const issued: IssuedAgentApiToken = {
      token: { ...sidecar, id: '0199aaaa-0000-7000-8000-00000000a003', name: 'production' },
      secret: SECRET,
    };
    issueAgentToken.mockImplementation(() => {
      tokens.list = [issued.token, ...tokens.list];
      return Promise.resolve(issued);
    });
    renderTokens();
    await screen.findByRole('list', { name: 'API tokens' });
    fireEvent.change(screen.getByLabelText('Name (optional)'), {
      target: { value: 'production' },
    });
    fireEvent.click(screen.getByRole('button', { name: /issue token/i }));
    await waitFor(() => expect(issueAgentToken).toHaveBeenCalledWith(ids.agent, 'production'));

    const shown = await screen.findByRole('status');
    expect(shown).toHaveTextContent('not shown again');
    expect(screen.getByLabelText('Token')).toHaveTextContent(SECRET);
    expect(screen.getByRole('button', { name: 'Copy token' })).toBeInTheDocument();
    // The form is away while the secret is up: one token at a time, read before the next.
    expect(screen.queryByRole('button', { name: /issue token/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByText(SECRET)).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole('list', { name: 'API tokens' })).toHaveTextContent('production'),
    );
    expect(screen.getByRole('button', { name: /issue token/i })).toBeInTheDocument();
  });

  it('revokes after a confirmation and refreshes the list', async () => {
    revokeAgentToken.mockImplementation(() => {
      tokens.list = tokens.list.map((t) =>
        t.id === sidecar.id ? { ...t, revoked_at: '2026-10-04T09:00:00Z' } : t,
      );
      return Promise.resolve();
    });
    renderTokens();
    await screen.findByRole('list', { name: 'API tokens' });
    fireEvent.click(screen.getByRole('button', { name: /revoke/i }));
    expect(revokeAgentToken).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));
    await waitFor(() => expect(revokeAgentToken).toHaveBeenCalledWith(ids.agent, sidecar.id));
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: /revoke/i })).not.toBeInTheDocument(),
    );
  });

  it('tells members that admins hold the tokens, and asks for none', () => {
    renderTokens(false);
    expect(screen.getByText(/admins issue and revoke/i)).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'API tokens' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /issue token/i })).not.toBeInTheDocument();
  });
});
