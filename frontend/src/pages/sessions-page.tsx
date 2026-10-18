import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { ListTreeIcon, NetworkIcon, RefreshCwIcon } from 'lucide-react';
import { useId, useState } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { TableSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NativeSelect, NativeSelectOption } from '@/components/ui/native-select';
import { agentsQuery } from '@/features/agents/api';
import { localToIso } from '@/features/secops/explorer';
import { sessionFilterParams, sessionsPage } from '@/features/sessions/api';
import { SessionsTable } from '@/features/sessions/components/sessions-table';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { qk } from '@/lib/query-keys';
import { RUN_STATUSES, type RunStatus } from '@/schemas/run';
import type { SessionFilter } from '@/schemas/session';
import type { Workspace } from '@/schemas/workspace';

function SessionList({ workspace }: { workspace: Workspace }) {
  const ids = { from: useId(), to: useId() };
  const [filter, setFilter] = useState<SessionFilter>({});
  const { data: agents = [] } = useQuery(agentsQuery(workspace.id));
  const params = sessionFilterParams(filter);
  const query = useInfiniteQuery({
    queryKey: qk.sessions.list(workspace.id, params),
    queryFn: ({ pageParam, signal }) => sessionsPage(workspace.id, filter, pageParam, signal),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
    refetchInterval: 15_000,
  });
  const rows = query.data?.pages.flatMap((p) => p.sessions) ?? [];
  const set = <K extends keyof SessionFilter>(key: K, value: SessionFilter[K]) =>
    setFilter((f) => ({ ...f, [key]: value }));
  return (
    <div className="space-y-4">
      <form
        className="grid gap-2 rounded-xl border bg-card p-3 text-xs sm:grid-cols-2 lg:grid-cols-5"
        aria-label="Filters"
        onSubmit={(e) => e.preventDefault()}
      >
        <label className="space-y-1">
          <span className="text-muted-foreground">Agent</span>
          <NativeSelect
            size="sm"
            className="w-full"
            value={filter.agent ?? ''}
            onChange={(e) => set('agent', e.target.value || undefined)}
          >
            <NativeSelectOption value="">any</NativeSelectOption>
            {agents.map((a) => (
              <NativeSelectOption key={a.id} value={a.id}>
                {a.name}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label className="space-y-1">
          <span className="text-muted-foreground">Status</span>
          <NativeSelect
            size="sm"
            className="w-full"
            value={filter.status ?? ''}
            onChange={(e) => set('status', (e.target.value || undefined) as RunStatus | undefined)}
          >
            <NativeSelectOption value="">any</NativeSelectOption>
            {RUN_STATUSES.map((s) => (
              <NativeSelectOption key={s} value={s}>
                {s}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </label>
        <label htmlFor={ids.from} className="space-y-1">
          <span className="text-muted-foreground">From</span>
          <Input
            id={ids.from}
            type="datetime-local"
            onChange={(e) => set('from', localToIso(e.target.value))}
            className="h-7 text-xs"
          />
        </label>
        <label htmlFor={ids.to} className="space-y-1">
          <span className="text-muted-foreground">To</span>
          <Input
            id={ids.to}
            type="datetime-local"
            onChange={(e) => set('to', localToIso(e.target.value))}
            className="h-7 text-xs"
          />
        </label>
        <div className="flex items-end justify-end gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={query.isFetching}
            onClick={() => void query.refetch()}
          >
            <RefreshCwIcon className={query.isFetching ? 'motion-safe:animate-spin' : undefined} />
            Refresh
          </Button>
        </div>
      </form>
      {query.isError && rows.length === 0 ? (
        <ErrorState
          title="Couldn’t load the sessions"
          error={query.error}
          onRetry={() => void query.refetch()}
        />
      ) : query.isPending ? (
        <TableSkeleton />
      ) : rows.length === 0 ? (
        <EmptyState
          icon={ListTreeIcon}
          title="No sessions yet"
          description="Runs of this workspace appear here as trees of nodes, agents and sub-agents, with their tokens and cost."
          action={
            <Button asChild>
              <Link to="/app">
                <NetworkIcon />
                Open a graph to run
              </Link>
            </Button>
          }
        />
      ) : (
        <>
          <SessionsTable rows={rows} />
          {query.hasNextPage && (
            <div className="flex justify-center">
              <Button
                variant="outline"
                size="sm"
                disabled={query.isFetchingNextPage}
                onClick={() => void query.fetchNextPage()}
              >
                Older sessions
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function SessionsPage() {
  const { current } = useCurrentWorkspace();
  return (
    <PageContainer>
      <Seo title="Sessions" noIndex />
      <PageHeader
        title="Sessions"
        description="Every run as a tree: its nodes, the agent each ran as, the sub-agents it spawned, and what each spent."
      />
      {!current ? (
        <TableSkeleton />
      ) : current.role === 'guest' ? (
        <EmptyState
          icon={ListTreeIcon}
          title="Members see the sessions"
          description="Ask an owner or admin of this workspace to make you a member."
        />
      ) : (
        <SessionList key={current.id} workspace={current} />
      )}
    </PageContainer>
  );
}
