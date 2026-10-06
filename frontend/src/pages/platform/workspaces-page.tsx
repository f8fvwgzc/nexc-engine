import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import { platformWorkspacesQuery } from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { WorkspaceSheet } from '@/features/platform/workspace-sheet';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';

export default function PlatformWorkspacesPage() {
  const { page, search, controls } = usePlatformPage();
  const { data, isPending, error } = useQuery(platformWorkspacesQuery(page));
  const [opened, setOpened] = useState<string | null>(null);
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <Seo title="Workspaces" noIndex />
      <PageHeader
        title="Workspaces"
        description="Every workspace on this installation, with who owns it and how big it is. Open one to see its members, give it an owner or delete it. Their content is not shown here."
      />
      {search}
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      ) : isPending ? (
        <Skeleton className="h-64 w-full rounded-lg" />
      ) : (
        <div className="overflow-hidden rounded-lg border">
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
                  className="flex min-h-10 w-full flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-left text-[13px] hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none"
                  onClick={() => setOpened(w.id)}
                >
                  <span className="min-w-0 flex-1 basis-40 truncate font-medium">{w.name}</span>
                  <span className="w-56 min-w-0">
                    <span className="block truncate">{w.owner_name ?? 'No owner'}</span>
                    <span className="block truncate text-xs text-muted-foreground">
                      {w.owner_email ?? ''}
                    </span>
                  </span>
                  <span className="w-16 text-right tabular-nums">{w.member_count}</span>
                  <span className="w-14 text-right tabular-nums">{w.team_count}</span>
                  <span className="w-14 text-right tabular-nums">{w.issue_count}</span>
                  <span className="w-14 text-right tabular-nums">{w.graph_count}</span>
                  <span className="w-24 text-right text-xs text-muted-foreground">
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
    </div>
  );
}
