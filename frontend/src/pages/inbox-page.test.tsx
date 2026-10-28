import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { HelmetProvider } from 'react-helmet-async';
import { MemoryRouter, Route, Routes, useParams } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import type { Notification } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import InboxPage from './inbox-page';

const at = '2026-10-20T09:00:00Z';

const workspace: Workspace = {
  id: '0199eeee-0000-7000-8000-000000000001',
  name: 'Acme',
  slug: 'acme',
  role: 'member',
  member_count: 3,
  created_at: at,
};

const mention: Notification = {
  id: '0199eeee-0000-7000-8000-000000000002',
  kind: 'mention',
  issue: { id: '0199eeee-0000-7000-8000-000000000003', identifier: 'ENG-12', title: 'Fix login' },
  approval: null,
  alert: null,
  actor: { user_id: '0199eeee-0000-7000-8000-000000000004', name: 'Grace Hopper' },
  emoji: null,
  created_at: at,
  read_at: null,
};

const reaction: Notification = {
  ...mention,
  id: '0199eeee-0000-7000-8000-000000000005',
  kind: 'reaction',
  emoji: '🎉',
};

/** What the inbox holds. */
let inbox: Notification[] = [mention];

vi.mock('@/lib/api/client', () => ({
  apiRequest: (path: string) => {
    if (path === '/workspaces') return Promise.resolve([workspace]);
    if (path.endsWith('/inbox')) return Promise.resolve(inbox);
    if (path.endsWith('/inbox/read')) return Promise.resolve([{ ...mention, read_at: at }]);
    return Promise.resolve([]);
  },
  apiSend: vi.fn(),
  apiDownload: vi.fn(),
}));

function Issue() {
  return <p>Opened {useParams().issueId}</p>;
}

function renderInbox() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <HelmetProvider>
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={['/app/inbox']}>
          <Routes>
            <Route path="/app/inbox" element={<InboxPage />} />
            <Route path="/app/issues/:issueId" element={<Issue />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>
    </HelmetProvider>,
  );
}

describe('a mention in the inbox', () => {
  it('says who mentioned you in which issue, and opens it', async () => {
    inbox = [mention];
    renderInbox();
    const entry = await screen.findByRole('button', { name: /mentioned you in ENG-12/ });
    expect(entry).toHaveTextContent('Grace Hopper mentioned you in ENG-12 Fix login');
    fireEvent.click(entry);
    expect(await screen.findByText(`Opened ${mention.issue?.id}`)).toBeInTheDocument();
  });
});

describe('a reaction in the inbox', () => {
  it('says who reacted to your comment with what, on which issue', async () => {
    inbox = [reaction];
    renderInbox();
    const entry = await screen.findByRole('button', { name: /reacted 🎉 to your comment on/ });
    expect(entry).toHaveTextContent('Grace Hopper reacted 🎉 to your comment on ENG-12 Fix login');
    fireEvent.click(entry);
    expect(await screen.findByText(`Opened ${reaction.issue?.id}`)).toBeInTheDocument();
  });
});
