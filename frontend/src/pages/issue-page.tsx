import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { ArrowLeftIcon, CircleDotIcon } from 'lucide-react';
import { useEffect, useEffectEvent } from 'react';
import { Link, useParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { IssueArtifacts } from '@/features/artifacts/issue-artifacts';
import { isTypingTarget } from '@/features/command/shortcuts';
import { issueQuery } from '@/features/issues/api';
import { IssueDetail, IssueDetailSkeleton } from '@/features/issues/issue-detail';
import { cameFromApp, useLeaveIssue } from '@/features/issues/links';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { ApiError } from '@/lib/api/errors';
import type { Issue } from '@/schemas/issue';

/** An issue as a list carries it: every field, but maybe not the description (lists are slim). */
type ListedIssue = Omit<Issue, 'description'> & { description?: string };

/** The copy of an issue in a list already read, shown while the issue's own copy loads. */
function listedIssue(queryClient: QueryClient, issueId: string): ListedIssue | undefined {
  const lists = queryClient.getQueriesData<ListedIssue[]>({
    predicate: (query) => query.queryKey[0] === 'issues' && query.queryKey[2] === 'list',
  });
  for (const [, issues] of lists) {
    const found = issues?.find((i) => i.id === issueId);
    if (found) return found;
  }
  return undefined;
}

function IssueView({ issueId }: { issueId: string }) {
  const queryClient = useQueryClient();
  const { current, all } = useCurrentWorkspace();
  const leave = useLeaveIssue();
  const {
    data: issue,
    error,
    refetch,
    isPlaceholderData,
  } = useQuery({
    ...issueQuery(issueId),
    // Read again on every visit: the list it was opened from may be minutes old, and its copy
    // shows until the fresh one lands.
    staleTime: 0,
    placeholderData: () => {
      const listed = listedIssue(queryClient, issueId);
      return listed && { ...listed, description: listed.description ?? '' };
    },
  });
  // A list's copy without a description: the field waits for the issue's own copy, so an empty
  // one is never shown, edited or saved over the real text.
  const descriptionPending =
    isPlaceholderData && listedIssue(queryClient, issueId)?.description === undefined;

  // Escape goes back to the list the issue was opened from, unless something else takes it (a
  // menu, a dialog, a field being typed in). Opened straight from a link, there is none.
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    if (e.key !== 'Escape' || e.defaultPrevented || isTypingTarget(e.target) || !cameFromApp()) {
      return;
    }
    leave();
  });
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey(e);
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, []);

  if (!issue) {
    if (error instanceof ApiError && error.status === 404) {
      return (
        <PageContainer>
          <EmptyState
            icon={CircleDotIcon}
            title="No such issue"
            headingLevel={1}
            description="It may have been deleted, or it belongs to a team you cannot see."
            action={
              <Button variant="outline" asChild>
                <Link to="/app/issues">
                  <ArrowLeftIcon />
                  All issues
                </Link>
              </Button>
            }
          />
        </PageContainer>
      );
    }
    if (error) {
      return (
        <PageContainer>
          <h1 className="sr-only">Issue</h1>
          <ErrorState
            title="Couldn’t load the issue"
            error={error}
            onRetry={() => void refetch()}
          />
        </PageContainer>
      );
    }
    return <IssueDetailSkeleton />;
  }
  // An issue linked from another of the member's workspaces is shown with that workspace's
  // teams, projects and members.
  const workspace = all.find((w) => w.id === issue.workspace_id) ?? current;
  if (!workspace) return <IssueDetailSkeleton />;
  return (
    <>
      <Seo title={`${issue.identifier} ${issue.title}`} noIndex />
      <IssueDetail
        key={issue.id}
        workspace={workspace}
        issue={issue}
        descriptionPending={descriptionPending}
        artifacts={<IssueArtifacts workspace={workspace} issue={issue} />}
      />
    </>
  );
}

/** One issue on its own page (`/app/issues/<id>`): everything about it, editable in place. */
export default function IssuePage() {
  const { issueId = '' } = useParams();
  return <IssueView key={issueId} issueId={issueId} />;
}
