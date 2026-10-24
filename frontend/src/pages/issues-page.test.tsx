import { render, screen } from '@testing-library/react';
import { createMemoryRouter, RouterProvider, useLocation, useParams } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { issuePath, legacyIssuePath } from '@/features/issues/links';

import IssuesPage from './issues-page';

const id = '6c08cc63-f95f-77d4-8576-23a9c41035ee';

function IssueProbe() {
  const { issueId } = useParams();
  const location = useLocation();
  return (
    <p data-testid="issue">
      {issueId} {location.search}
    </p>
  );
}

describe('old issue links', () => {
  it('name the issue page they stand for', () => {
    expect(issuePath(id)).toBe(`/app/issues/${id}`);
    expect(legacyIssuePath(new URLSearchParams(`issue=${id}`))).toBe(`/app/issues/${id}`);
    expect(legacyIssuePath(new URLSearchParams('team=eng'))).toBeNull();
    expect(legacyIssuePath(new URLSearchParams('issue='))).toBeNull();
  });

  it('open the issue on its own page, in place of the old address', () => {
    const router = createMemoryRouter(
      [
        { path: '/app/inbox', element: <p>inbox</p> },
        { path: '/app/issues', element: <IssuesPage /> },
        { path: '/app/issues/:issueId', element: <IssueProbe /> },
      ],
      { initialEntries: ['/app/inbox', `/app/issues?issue=${id}`], initialIndex: 1 },
    );
    render(<RouterProvider router={router} />);
    expect(screen.getByTestId('issue')).toHaveTextContent(id);
    expect(router.state.location.pathname).toBe(`/app/issues/${id}`);
    // Replaced, not pushed: going back skips the old address.
    expect(router.state.historyAction).toBe('REPLACE');
  });
});
