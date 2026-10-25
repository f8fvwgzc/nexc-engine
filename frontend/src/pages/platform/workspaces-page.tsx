import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { ErrorState } from '@/components/custom-ui/error-state';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { ListSkeleton } from '@/components/layout/page-skeleton';
import { platformWorkspacesQuery } from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { WorkspaceSheet } from '@/features/platform/workspace-sheet';
import { formatRelative } from '@/lib/format';

export default function PlatformWorkspacesPage() {
  const { page, search, controls } = usePlatformPage();
  const { data, isPending, error, refetch } = useQuery(platformWorkspacesQuery(page));
  const [opened, setOpened] = useState<string | null>(null);
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <PageContainer width="narrow">
      <Seo title="Workspaces" noIndex />
      <PageHeader
        title="Workspaces"
        description="Every workspace on this installation, with who owns it and how big it is. Open one to see its members, give it an owner or delete it. Their content is not shown here."
      />
      {search}
      {error ? (
        <ErrorState
          title="Couldn’t load the workspaces"
          error={error}
          onRetry={() => void refetch()}
        />
      ) : isPending ? (
        <ListSkeleton rows={8} label="Loading the workspaces" />
      ) : (
        <div className="overflow-hidden rounded-lg border motion-safe:animate-fade-in">
          <div className="hidden h-8 items-center gap-3 border-b bg-muted/40 px-3 text-xs text-muted-foreground sm:flex">
            <span className="flex-1">Workspace</span>
            <span className="w-56">Owner</span>
            <span className="w-16 text-right">Members</span>
            <span className="w-14 text-right">Teams</span>
            <span className="w-14 text-right">Issues</span>
            <span className="w-14 text-right">Graphs</span>
            <span className="w-24 text-right">Created</span>
          </div>
          <ul className="divide-y">
            {rows.length === 0 && (
              <li className="px-3 py-6 text-center text-[13px] text-muted-foreground">
                No workspace matches.
              </li>
            )}
            {rows.map((w) => (
              <li key={w.id}>
                <button
                  type="button"
                  className="flex min-h-10 w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left text-[13px] hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none focus-visible:ring-inset"
                  onClick={() => setOpened(w.id)}
                >
                  <span className="min-w-0 flex-1 basis-40 truncate font-medium" title={w.name}>
                    {w.name}
                  </span>
                  <span className="w-56 min-w-0">
                    <span className="block truncate">{w.owner_name ?? 'No owner'}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {w.owner_email ?? ''}
                    </span>
                  </span>
                  {/* The columns have headings from sm up; below, each count says what it counts. */}
                  <span className="w-full text-xs text-muted-foreground tabular-nums sm:hidden">
                    {w.member_count} members · {w.team_count} teams · {w.issue_count} issues ·{' '}
                    {w.graph_count} graphs · created {formatRelative(w.created_at)}
                  </span>
                  <span className="hidden w-16 text-right tabular-nums sm:block">
                    {w.member_count}
                  </span>
                  <span className="hidden w-14 text-right tabular-nums sm:block">
                    {w.team_count}
                  </span>
                  <span className="hidden w-14 text-right tabular-nums sm:block">
                    {w.issue_count}
                  </span>
                  <span className="hidden w-14 text-right tabular-nums sm:block">
                    {w.graph_count}
                  </span>
                  <span className="hidden w-24 text-right text-xs text-muted-foreground sm:block">
                    {formatRelative(w.created_at)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {controls(data?.length ?? 0)}
      <WorkspaceSheet id={opened} onClose={() => setOpened(null)} />
    </PageContainer>
  );
}
