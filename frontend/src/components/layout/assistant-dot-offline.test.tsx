import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import type * as MotionPresets from '@/components/custom-ui/motion-presets';
import { conversationQuery } from '@/features/workspaces/api';
import type { ConversationDetail } from '@/schemas/assistant';
import type { Workspace } from '@/schemas/workspace';
import { useAssistantStore } from '@/stores/assistant-store';

import { AssistantDot } from './assistant-dot';

/*
 * The panel when the chunks it loads late cannot be had (offline, or a tab left open across a
 * deploy that removed them): motion's animation code and the Markdown renderer both fail.
 */

const { workspace } = vi.hoisted(() => ({
  workspace: {
    id: '0199eeee-0000-7000-8000-000000000001',
    name: 'Acme',
    slug: 'acme',
    role: 'member',
    member_count: 2,
    created_at: '2026-10-20T09:00:00Z',
  } satisfies Workspace,
}));

vi.mock('@/features/workspaces/use-current-workspace', () => ({
  useCurrentWorkspace: () => ({ current: workspace, all: [workspace] }),
}));
vi.mock('./use-page-context', () => ({
  usePageContext: () => ({ path: '/app/issues', title: 'Issues' }),
}));
vi.mock('@/components/custom-ui/motion-presets', async (importOriginal) => ({
  ...(await importOriginal<typeof MotionPresets>()),
  loadAnimation: () => Promise.reject(new TypeError('Failed to fetch dynamically imported module')),
}));
vi.mock('@/components/custom-ui/markdown', () => {
  throw new TypeError('Failed to fetch dynamically imported module');
});

const CONVERSATION_ID = '0199eeee-0000-7000-8000-0000000000c1';

const detail: ConversationDetail = {
  conversation: {
    id: CONVERSATION_ID,
    workspace_id: workspace.id,
    title: 'Tenders',
    page_path: '/app/issues',
    page_title: 'Issues',
    message_count: 2,
    created_at: '2026-10-20T09:00:00Z',
    updated_at: '2026-10-20T09:01:00Z',
  },
  messages: [
    {
      id: '0199eeee-0000-7000-8000-0000000000d1',
      role: 'user',
      content: 'Which tenders close this month?',
      outcome: null,
      page_path: '/app/issues',
      page_title: 'Issues',
      created_at: '2026-10-20T09:00:00Z',
    },
    {
      id: '0199eeee-0000-7000-8000-0000000000d2',
      role: 'assistant',
      content: 'Two close before the 31st: **Bavaria** and **Lyon**.',
      outcome: { created: [], skipped: [], memories_used: 0, proposals: [] },
      page_path: '/app/issues',
      page_title: 'Issues',
      created_at: '2026-10-20T09:01:00Z',
    },
  ],
};

function renderPanel(client = new QueryClient()) {
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AssistantDot />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

/** Lets the failed imports settle. */
const settle = () => act(async () => new Promise((resolve) => setTimeout(resolve, 0)));

beforeAll(() => {
  Element.prototype.scrollIntoView = () => undefined;
});

afterEach(() => {
  useAssistantStore.setState({ open: false, workspaceId: null, conversationId: null });
});

describe('the assistant panel without its late chunks', () => {
  it('opens where its animation would end instead of staying at width 0', async () => {
    renderPanel();
    await settle();
    act(() => useAssistantStore.setState({ open: true }));

    await waitFor(() => {
      const panel = screen.getByRole('complementary', { name: 'Assistant' });
      expect(panel.style.width).toBe('448px');
      expect(panel.style.opacity).toBe('1');
    });
    // What rises into it shows too.
    const greeting = screen.getByText('How can I help in Acme?').parentElement;
    expect(greeting?.style.opacity).toBe('1');
  });

  it('shows replies as plain text when the Markdown renderer cannot load', async () => {
    const client = new QueryClient();
    client.setQueryData(conversationQuery(workspace.id, CONVERSATION_ID).queryKey, detail);
    useAssistantStore.setState({
      open: true,
      workspaceId: workspace.id,
      conversationId: CONVERSATION_ID,
    });
    renderPanel(client);
    await settle();

    expect(
      await screen.findByText('Two close before the 31st: **Bavaria** and **Lyon**.'),
    ).toBeVisible();
    expect(screen.getByText('Which tenders close this month?')).toBeVisible();
  });
});
