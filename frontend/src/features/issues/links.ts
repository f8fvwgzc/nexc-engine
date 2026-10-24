import { useNavigate } from 'react-router-dom';

import { PRIORITY_LABEL, type Issue } from '@/schemas/issue';

/** The page of one issue. */
export function issuePath(issueId: string): string {
  return `/app/issues/${issueId}`;
}

/**
 * Where an old issue link lands: `/app/issues?issue=<id>` (inbox entries and audit rows written
 * before issues had pages of their own) opens that issue's page. Null when the address is not one.
 */
export function legacyIssuePath(params: URLSearchParams): string | null {
  const id = params.get('issue');
  return id ? issuePath(id) : null;
}

/**
 * What a row or card linking to an issue says on hover. Its link covers the whole row, so the
 * glyphs under it (state, priority, assignee, due date) cannot show tooltips of their own.
 */
export function issueLinkTitle(issue: Issue): string {
  return [
    `${issue.identifier} ${issue.title}`,
    issue.state.name,
    PRIORITY_LABEL[issue.priority] ?? PRIORITY_LABEL[0],
    issue.assignee?.name ?? 'Unassigned',
    issue.due_date ? `Due ${issue.due_date}` : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

/**
 * Whether this page was reached from another page of the app, so going back stays in the app.
 * The router numbers a visit's history entries from 0 and keeps the number through redirects (a
 * sign-in's `next`, an old `?issue=` link), which a location's key does not: every redirect gets a
 * fresh one.
 */
export function cameFromApp(): boolean {
  const state = window.history.state as { idx?: unknown } | null;
  return typeof state?.idx === 'number' && state.idx > 0;
}

/**
 * Leaving an issue's page: back to where the member came from in the app (a list, a board, the
 * inbox), or to the issues list when the page was opened straight from a link.
 */
export function useLeaveIssue(): () => void {
  const navigate = useNavigate();
  return () => {
    if (cameFromApp()) void navigate(-1);
    else void navigate('/app/issues', { replace: true });
  };
}
