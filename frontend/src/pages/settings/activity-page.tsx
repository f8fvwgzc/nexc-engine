import { useQuery } from '@tanstack/react-query';
import { CalendarDaysIcon } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Input } from '@/components/ui/input';
import { Skeleton } from '@/components/ui/skeleton';
import { timelineDaysQuery, timelineQuery } from '@/features/insight/api';
import { DaySummaryCard } from '@/features/insight/day-summary';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import type { TimelineEntry } from '@/schemas/insight';
import { isWorkspaceAdmin, type Workspace } from '@/schemas/workspace';

const today = () => new Date().toISOString().slice(0, 10);
const timeFormat = new Intl.DateTimeFormat('en', {
  hour: '2-digit',
  minute: '2-digit',
  timeZone: 'UTC',
  hour12: false,
});
const dayFormat = new Intl.DateTimeFormat('en', {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
});

/** What the actor did, read before the entry's title. Unknown kinds read as they are. */
const VERB: Record<string, string> = {
  issue_created: 'filed',
  issue_comment: 'commented on',
  issue_state: 'moved',
  issue_assignee: 'reassigned',
  issue_priority: 'reprioritised',
  issue_title: 'renamed',
  graph_created: 'created the graph',
  run_succeeded: 'ran',
  run_failed: 'ran (failed)',
  run_cancelled: 'cancelled a run of',
  run_running: 'is running',
  run_queued: 'queued a run of',
  document_added: 'added the document',
  memories_learned: 'Memory grew from',
  member_added: 'added',
  member_invited: 'invited',
  member_role_changed: 'changed the role of',
  member_removed: 'removed',
  invite_withdrawn: 'withdrew an',
  team_created: 'created the team',
  team_updated: 'changed the team',
  team_deleted: 'deleted the team',
  team_member_set: 'put on a team:',
  team_member_removed: 'took off a team:',
  credential_set: 'set the',
  credential_removed: 'removed the',
  guardrails_changed: 'changed the',
  knowledge_changed: 'changed the',
  workspace_transferred: 'started copying the',
  platform_owner_assigned: 'gave ownership of the workspace to',
  label_deleted: 'deleted the label',
  workspace_renamed: 'renamed the workspace to',
};

/** Where an entry's subject can be opened, if anywhere. */
function href(entry: TimelineEntry): string | null {
  if (!entry.entity_id) return null;
  if (entry.entity_type === 'issue') return `/app/issues?issue=${entry.entity_id}`;
  if (entry.entity_type === 'graph') return `/app/graphs/${entry.entity_id}`;
  if (entry.entity_type === 'document') return '/app/settings/knowledge';
  return null;
}

function Day({ workspace, day }: { workspace: Workspace; day: string }) {
  const { data: entries, isPending, error } = useQuery(timelineQuery(workspace.id, day));
  if (isPending) {
    return (
      <div className="space-y-2" aria-busy>
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-8 w-full" />
        ))}
      </div>
    );
  }
  if (error) {
    return (
      <p role="alert" className="text-sm text-destructive">
        {errorMessage(error)}
      </p>
    );
  }
  if (entries.length === 0) {
    return (
      <EmptyState
        icon={CalendarDaysIcon}
        title="Nothing happened that day"
        description="Pick another day, or one of the busy ones above."
      />
    );
  }
  return (
    <div className="space-y-4">
      <DaySummaryCard workspaceId={workspace.id} day={day} entries={entries.length} />
      <Entries entries={entries} />
    </div>
  );
}

function Entries({ entries }: { entries: TimelineEntry[] }) {
  return (
    <ol className="divide-y rounded-lg border">
      {entries.map((entry, index) => {
        const to = href(entry);
        const title = <span className="font-medium">{entry.title}</span>;
        return (
          <li
            key={`${entry.at}:${entry.kind}:${index}`}
            className="flex items-start gap-2.5 px-3 py-2 text-[13px]"
          >
            <time
              dateTime={entry.at}
              className="w-10 shrink-0 pt-0.5 text-xs text-muted-foreground tabular-nums"
            >
              {timeFormat.format(new Date(entry.at))}
            </time>
            <PersonGlyph name={entry.actor ?? undefined} className="mt-0.5" />
            <p className="min-w-0 flex-1 break-words">
              {entry.actor && <span>{entry.actor} </span>}
              <span className="text-muted-foreground">{VERB[entry.kind] ?? entry.kind}</span>{' '}
              {to ? (
                <Link to={to} className="underline-offset-2 hover:underline">
                  {title}
                </Link>
              ) : (
                title
              )}
              {entry.detail && <span className="text-muted-foreground"> · {entry.detail}</span>}
            </p>
          </li>
        );
      })}
    </ol>
  );
}

function Activity({ workspace }: { workspace: Workspace }) {
  const [day, setDay] = useState(today);
  const { data: days = [] } = useQuery(timelineDaysQuery(workspace.id, 30));
  const most = Math.max(1, ...days.map((d) => d.events));
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end gap-3">
        <label className="space-y-1 text-xs text-muted-foreground">
          Day (UTC)
          <Input
            type="date"
            value={day}
            max={today()}
            className="h-8 w-40 text-[13px] text-foreground"
            onChange={(e) => setDay(e.target.value || today())}
          />
        </label>
        {days.length > 0 && (
          <ul aria-label="Busy days in the last 30" className="flex flex-wrap items-end gap-1">
            {days
              .slice()
              .reverse()
              .map((d) => (
                <li key={d.day}>
                  <button
                    type="button"
                    aria-pressed={d.day === day}
                    title={`${dayFormat.format(new Date(`${d.day}T00:00:00Z`))}: ${d.events} events`}
                    onClick={() => setDay(d.day)}
                    className="flex h-10 w-4 items-end rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring aria-pressed:[&>span]:bg-primary"
                  >
                    <span
                      className="w-full rounded-sm bg-muted-foreground/40"
                      style={{ height: `${Math.max(12, (d.events / most) * 100)}%` }}
                    />
                    <span className="sr-only">
                      {d.day}: {d.events} events
                    </span>
                  </button>
                </li>
              ))}
          </ul>
        )}
      </div>
      <Day key={day} workspace={workspace} day={day} />
    </div>
  );
}

export default function ActivityPage() {
  const { current } = useCurrentWorkspace();
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="Activity" noIndex />
      <PageHeader
        title="Activity"
        description="What happened in the workspace on a day: people, teams, issues, graphs, runs, documents and memory, newest first, with a summary the workspace's AI writes when you ask."
      />
      {!current ? (
        <PageSkeleton />
      ) : isWorkspaceAdmin(current.role) ? (
        <Activity key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={CalendarDaysIcon}
          title="Only admins see the workspace's activity"
          description="It shows every team's work. Ask an owner or admin."
        />
      )}
    </div>
  );
}
