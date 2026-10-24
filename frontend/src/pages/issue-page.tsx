import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { ArrowLeftIcon, CircleDotIcon } from 'lucide-react';
import { useEffect, useEffectEvent } from 'react';
import { Link, useParams } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { IssueArtifacts } from '@/features/artifacts/issue-artifacts';
import { isTypingTarget } from '@/features/command/shortcuts';
import { issueQuery } from '@/features/issues/api';
import { IssueDetail } from '@/features/issues/issue-detail';
import { cameFromApp, useLeaveIssue } from '@/features/issues/links';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { ApiError } from '@/lib/api/errors';
import type { Issue } from '@/schemas/issue';

/** The copy of an issue in a list already read, shown while the issue's own copy loads. */
function listedIssue(queryClient: QueryClient, issueId: string): Issue | undefined {
  const lists = queryClient.getQueriesData<Issue[]>({
    predicate: (query) => query.queryKey[0] === 'issues' && query.queryKey[2] === 'list',
  });
  for (const [, issues] of lists) {
    const found = issues?.find((i) => i.id === issueId);
    if (found) return found;
  }
  return undefined;
}

/** The issue page's frame while the issue loads: its top bar, main column and properties. */
function IssueSkeleton() {
  return (
    <div
      role="status"
      aria-busy
      aria-label="Loading the issue"
      className="flex flex-col md:min-h-0 md:flex-1"
    >
      <div className="flex h-11 shrink-0 items-center border-b px-4 sm:px-6">
        <Skeleton className="h-4 w-40" />
      </div>
      <div className="flex flex-col md:min-h-0 md:flex-1 md:flex-row">
        <div className="mx-auto w-full max-w-[52rem] flex-1 space-y-3 px-4 py-6 sm:px-8">
          <Skeleton className="h-7 w-2/3" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-5/6" />
        </div>
        <div className="space-y-2 border-t p-4 md:w-72 md:border-t-0 md:border-l">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-8 w-full" />
          ))}
        </div>
      </div>
    </div>
  );
}

function IssueView({ issueId }: { issueId: string }) {
  const queryClient = useQueryClient();
  const { current, all } = useCurrentWorkspace();
  const leave = useLeaveIssue();
  const {
    data: issue,
    error,
    refetch,
  } = useQuery({
    ...issueQuery(issueId),
    // Read again on every visit: the list it was opened from may be minutes old, and its copy
    // shows until the fresh one lands.
    staleTime: 0,
    placeholderData: () => listedIssue(queryClient, issueId),
  });

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
    return <IssueSkeleton />;
  }
  // An issue linked from another of the member's workspaces is shown with that workspace's
  // teams, projects and members.
  const workspace = all.find((w) => w.id === issue.workspace_id) ?? current;
  if (!workspace) return <PageSkeleton />;
  return (
    <>
      <Seo title={`${issue.identifier} ${issue.title}`} noIndex />
      <IssueDetail
        key={issue.id}
        workspace={workspace}
        issue={issue}
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
