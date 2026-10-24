import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { createMemoryRouter, RouterProvider } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import type { Issue } from '@/schemas/issue';

import { cameFromApp, issueLinkTitle, issuePath, useLeaveIssue } from './links';

const id = '6c08cc63-f95f-77d4-8576-23a9c41035ee';

/** Stands for the router's own entry in the browser's history: `idx` counts from the visit's first page. */
function atHistoryIndex(idx: number | undefined) {
  window.history.replaceState(idx === undefined ? null : { idx }, '');
}

afterEach(() => atHistoryIndex(undefined));

describe('cameFromApp', () => {
  it('is true only past the first page of the visit', () => {
    atHistoryIndex(undefined);
    expect(cameFromApp()).toBe(false);
    // The first page, and a redirect from it (which keeps its number).
    atHistoryIndex(0);
    expect(cameFromApp()).toBe(false);
    atHistoryIndex(2);
    expect(cameFromApp()).toBe(true);
  });
});

function LeaveButton() {
  const leave = useLeaveIssue();
  return (
    <button type="button" onClick={leave}>
      Leave
    </button>
  );
}

function openIssue() {
  const router = createMemoryRouter(
    [
      { path: '/app/inbox', element: <p>inbox</p> },
      { path: '/app/issues', element: <p>issues</p> },
      { path: '/app/issues/:issueId', element: <LeaveButton /> },
    ],
    { initialEntries: ['/app/inbox', issuePath(id)], initialIndex: 1 },
  );
  render(<RouterProvider router={router} />);
  return router;
}

describe('leaving an issue', () => {
  it('goes back to the page of the app it was opened from', async () => {
    atHistoryIndex(1);
    const router = openIssue();
    fireEvent.click(screen.getByRole('button', { name: 'Leave' }));
    await waitFor(() => expect(router.state.location.pathname).toBe('/app/inbox'));
  });

  it('opened straight from a link (even through a redirect), goes to the issues list', async () => {
    atHistoryIndex(0);
    const router = openIssue();
    fireEvent.click(screen.getByRole('button', { name: 'Leave' }));
    await waitFor(() => expect(router.state.location.pathname).toBe('/app/issues'));
    expect(router.state.historyAction).toBe('REPLACE');
  });
});

describe('issueLinkTitle', () => {
  const at = '2026-10-20T09:00:00Z';
  const issue: Issue = {
    id,
    workspace_id: '0199bbbb-0000-7000-8000-00000000f001',
    team_id: '0199bbbb-0000-7000-8000-00000000f002',
    identifier: 'ENG-7',
    number: 7,
    title: 'Pick the wrist bus',
    description: '',
    state: {
      id: '0199bbbb-0000-7000-8000-00000000f003',
      team_id: '0199bbbb-0000-7000-8000-00000000f002',
      name: 'In Progress',
      category: 'started',
      color: '#f59e0b',
      position: 2,
    },
    priority: 2,
    labels: [],
    parent: null,
    cycle: null,
    sub_issues: { total: 0, closed: 0 },
    assignee: { user_id: '0199bbbb-0000-7000-8000-00000000f004', name: 'Ada Lovelace' },
    agent_id: null,
    project_id: null,
    graph_id: null,
    creator_id: null,
    due_date: '2026-10-30',
    github_url: null,
    created_at: at,
    updated_at: at,
    completed_at: null,
  };

  it('names what the glyphs under a row show', () => {
    expect(issueLinkTitle(issue)).toBe(
      'ENG-7 Pick the wrist bus · In Progress · High · Ada Lovelace · Due 2026-10-30',
    );
    expect(issueLinkTitle({ ...issue, priority: 0, assignee: null, due_date: null })).toBe(
      'ENG-7 Pick the wrist bus · In Progress · No priority · Unassigned',
    );
  });
});
