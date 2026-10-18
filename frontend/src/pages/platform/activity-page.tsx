import { useQuery } from '@tanstack/react-query';
import { HistoryIcon, UsersIcon } from 'lucide-react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { ErrorState } from '@/components/custom-ui/error-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageContainer } from '@/components/custom-ui/page-container';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import {
  platformEventsQuery,
  type PlatformAction,
  type PlatformEvent,
} from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { formatDateTime, formatRelative } from '@/lib/format';

/** What the administrator did, to be followed by the entry's subject. */
const VERB: Record<PlatformAction, string> = {
  role_changed: 'changed the platform role of',
  account_suspended: 'suspended',
  account_reactivated: 'reactivated',
  owner_assigned: 'assigned an owner to',
  workspace_deleted: 'deleted the workspace',
  account_erased: 'erased',
  password_reset_issued: 'issued a password reset link for',
  two_factor_reset: 'turned off two-factor sign-in for',
  identity_unlinked: 'unlinked a single sign-on identity from',
};

function Entry({ event }: { event: PlatformEvent }) {
  // The log keeps "Name <e-mail>"; the name alone reads better in a sentence.
  const actor = event.actor_name.replace(/\s*<[^>]*>$/, '') || 'An administrator who left';
  return (
    <li className="flex items-center gap-2.5 px-3 py-2 text-[13px]">
      <PersonGlyph name={actor} />
      <p className="min-w-0 flex-1">
        <span className="font-medium" title={event.actor_name}>
          {actor}
        </span>{' '}
        <span className="text-muted-foreground">{VERB[event.action]}</span>{' '}
        <span className="break-words">{event.subject}</span>
        {event.detail && <span className="text-muted-foreground"> · {event.detail}</span>}
      </p>
      <time
        dateTime={event.created_at}
        title={formatDateTime(event.created_at)}
        className="shrink-0 text-xs text-muted-foreground tabular-nums"
      >
        {formatRelative(event.created_at)}
      </time>
    </li>
  );
}

export default function PlatformActivityPage() {
  const { page, search, controls } = usePlatformPage('Search the log…');
  const { data, isPending, error, refetch } = useQuery(platformEventsQuery(page));
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <PageContainer width="narrow">
      <Seo title="Activity" noIndex />
      <PageHeader
        title="Activity"
        description="Everything platform administrators did from this console: role changes, suspensions, owners they assigned, workspaces they deleted and accounts they erased. Entries cannot be edited or removed here."
      />
      {search}
      {error ? (
        <ErrorState title="Couldn’t load the log" error={error} onRetry={() => void refetch()} />
      ) : isPending ? (
        <Skeleton className="h-64 w-full rounded-lg" />
      ) : rows.length === 0 && !page.q ? (
        <EmptyState
          icon={HistoryIcon}
          title="Nothing done from the console yet"
          description="Role changes, suspensions, owners assigned and accounts erased are logged here as they happen."
          action={
            <Button variant="outline" asChild>
              <Link to="/app/platform/users">
                <UsersIcon />
                Open Accounts
              </Link>
            </Button>
          }
        />
      ) : (
        <ul className="divide-y overflow-hidden rounded-lg border">
          {rows.length === 0 && (
            <li className="px-3 py-6 text-center text-[13px] text-muted-foreground">
              Nothing in the log matches.
            </li>
          )}
          {rows.map((event) => (
            <Entry key={event.id} event={event} />
          ))}
        </ul>
      )}
      {controls(data?.length ?? 0)}
    </PageContainer>
  );
}
