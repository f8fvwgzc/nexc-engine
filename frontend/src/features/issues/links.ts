import { useLocation, useNavigate } from 'react-router-dom';

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
 * Leaving an issue's page: back to where the member came from in the app (a list, a board, the
 * inbox), or to the issues list when the page was opened straight from a link. `cameFromApp`
 * says whether there is such a page to go back to.
 */
export function useLeaveIssue(): { cameFromApp: boolean; leave: () => void } {
  const navigate = useNavigate();
  // The first page of a visit has the default key; every page reached from it has its own.
  const cameFromApp = useLocation().key !== 'default';
  const leave = () => {
    if (cameFromApp) void navigate(-1);
    else void navigate('/app/issues', { replace: true });
  };
  return { cameFromApp, leave };
}
