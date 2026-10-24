import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import type { Workspace } from '@/schemas/workspace';
import { useAssistantStore } from '@/stores/assistant-store';

import { AssistantDot } from './assistant-dot';

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
  usePageContext: () => ({ path: '/app/issues/1', title: 'Issues › ENG-1 Pick the wrist bus' }),
}));

beforeAll(() => {
  // jsdom lays nothing out, so it has no scrolling to the end of the conversation.
  Element.prototype.scrollIntoView = () => undefined;
});

afterEach(() => {
  useAssistantStore.setState({ open: false, workspaceId: null, conversationId: null });
});

describe('the assistant panel', () => {
  it('takes Escape for itself when it closes, so the page under it does not act on it too', () => {
    useAssistantStore.setState({ open: true });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <AssistantDot />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    // The issue page goes back to its list on an Escape no one else took.
    const seen: boolean[] = [];
    const page = (e: KeyboardEvent) => seen.push(e.defaultPrevented);
    window.addEventListener('keydown', page);
    try {
      fireEvent.keyDown(screen.getByRole('button', { name: 'New conversation' }), {
        key: 'Escape',
      });
    } finally {
      window.removeEventListener('keydown', page);
    }
    expect(seen).toEqual([true]);
    expect(useAssistantStore.getState().open).toBe(false);
  });
});
