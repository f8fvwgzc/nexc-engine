import { useQuery } from '@tanstack/react-query';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';

import { activityQuery, type AccountEvent } from '../api';

/** What each entry says, and whether it deserves a second look. */
const KIND: Record<AccountEvent['kind'], { label: string; alert?: boolean }> = {
  registered: { label: 'Account created' },
  signed_in: { label: 'Signed in' },
  sign_in_failed: { label: 'Sign-in failed', alert: true },
  password_changed: { label: 'Password changed' },
  password_reset: { label: 'Password set', alert: true },
  reset_link_issued: { label: 'Password reset link created', alert: true },
  sessions_ended: { label: 'Signed out everywhere' },
  suspended: { label: 'Account suspended', alert: true },
  reactivated: { label: 'Account reactivated' },
  role_changed: { label: 'Platform role changed', alert: true },
};

/**
 * The record of how the account was got into and what changed about that: for its holder to
 * notice what they did not do themselves.
 */
export function ActivityCard() {
  const { data: events, isPending, error } = useQuery(activityQuery());
  return (
    <GlassCard id="security-activity" className="scroll-mt-20 space-y-3 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Security activity</h2>
        <p className="text-sm text-muted-foreground">
          Sign-ins and changes to how you get in, kept for 180 days. If something here was not you,
          change your password and sign out everywhere.
        </p>
      </div>
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(error)}
        </p>
      ) : isPending ? (
        <Skeleton className="h-32 w-full rounded-lg" />
      ) : events.length === 0 ? (
        <p className="text-sm text-muted-foreground">Nothing has been recorded yet.</p>
      ) : (
        <ul className="divide-y rounded-lg border text-[13px]">
          {events.map((event) => {
            const kind = KIND[event.kind];
            return (
              <li
                key={event.id}
                className="flex flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-2"
              >
                <span className={kind.alert ? 'font-medium' : undefined}>{kind.label}</span>
                {event.detail && <span className="text-muted-foreground">{event.detail}</span>}
                {event.ip && (
                  <span className="font-mono text-xs text-muted-foreground">from {event.ip}</span>
                )}
                <time
                  dateTime={event.created_at}
                  title={formatDateTime(event.created_at)}
                  className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums"
                >
                  {formatRelative(event.created_at)}
                </time>
              </li>
            );
          })}
        </ul>
      )}
    </GlassCard>
  );
}
