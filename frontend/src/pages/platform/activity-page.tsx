import { useQuery } from '@tanstack/react-query';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
import { Skeleton } from '@/components/ui/skeleton';
import {
  platformEventsQuery,
  type PlatformAction,
  type PlatformEvent,
} from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';

/** What the administrator did, to be followed by the entry's subject. */
const VERB: Record<PlatformAction, string> = {
  role_changed: 'changed the platform role of',
  account_suspended: 'suspended',
  account_reactivated: 'reactivated',
  owner_assigned: 'assigned an owner to',
  workspace_deleted: 'deleted the workspace',
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
  const { data, isPending, error } = useQuery(platformEventsQuery(page));
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <Seo title="Activity" noIndex />
      <PageHeader
        title="Activity"
        description="Everything platform administrators did from this console: role changes, suspensions, owners they assigned and workspaces they deleted. Entries cannot be edited or removed here."
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
              {page.q
                ? 'Nothing in the log matches.'
                : 'Nothing has been done from the console yet.'}
            </li>
          )}
          {rows.map((event) => (
            <Entry key={event.id} event={event} />
          ))}
        </ul>
      )}
      {controls(data?.length ?? 0)}
    </div>
  );
}
