import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { platformUsersQuery, setPlatformRole, type PlatformUser } from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { useAuthStore } from '@/stores/auth-store';

export default function PlatformUsersPage() {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const { page, search, controls } = usePlatformPage();
  const { data, isPending, error } = useQuery(platformUsersQuery(page));
  const change = useMutation({
    mutationFn: (user: PlatformUser) =>
      setPlatformRole(user.id, user.role === 'admin' ? 'user' : 'admin'),
    meta: { successMessage: 'Platform role changed' },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['platform', 'users'] }),
  });
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <Seo title="Accounts" noIndex />
      <PageHeader
        title="Accounts"
        description="Everyone who registered. A platform administrator sees this console; what someone may do inside a workspace is set in that workspace, not here."
      />
      {search}
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      ) : isPending ? (
        <Skeleton className="h-64 w-full rounded-lg" />
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border">
          {rows.length === 0 && (
            <li className="px-3 py-6 text-center text-[13px] text-muted-foreground">
              No account matches.
            </li>
          )}
          {rows.map((u) => (
            <li
              key={u.id}
              className="flex min-h-11 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-[13px]"
            >
              <PersonGlyph name={u.name} />
              <span className="min-w-0 flex-1 basis-48">
                <span className="block truncate font-medium">{u.name}</span>
                <span className="block truncate text-xs text-muted-foreground">{u.email}</span>
              </span>
              <span className="w-40 text-xs text-muted-foreground">
                {u.workspace_count} {u.workspace_count === 1 ? 'workspace' : 'workspaces'}, owns{' '}
                {u.owned_count}
              </span>
              {u.locked && <Badge variant="destructive">Locked</Badge>}
              <Badge
                variant={u.role === 'admin' ? 'default' : 'outline'}
                className="w-28 justify-center"
              >
                {u.role === 'admin' ? 'Platform admin' : 'User'}
              </Badge>
              <span className="hidden w-24 text-right text-xs text-muted-foreground sm:block">
                {formatRelative(u.created_at)}
              </span>
              <Button
                variant="ghost"
                size="sm"
                className="w-32"
                disabled={u.id === me || change.isPending}
                title={u.id === me ? 'Nobody changes their own platform role' : undefined}
                onClick={() => change.mutate(u)}
              >
                {u.role === 'admin' ? 'Make user' : 'Make admin'}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {controls(data?.length ?? 0)}
    </div>
  );
}
