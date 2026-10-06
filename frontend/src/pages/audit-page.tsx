import { useInfiniteQuery } from '@tanstack/react-query';
import { ScrollTextIcon } from 'lucide-react';

import { EmptyState } from '@/components/custom-ui/empty-state';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { AUDIT_PAGE_SIZE, auditPage } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import type { AuditAction, AuditEntry } from '@/schemas/audit';
import type { Workspace } from '@/schemas/workspace';

/** What the actor did, to be followed by the entry's subject. */
const VERB: Record<AuditAction, string> = {
  workspace_renamed: 'renamed the workspace to',
  member_added: 'added',
  member_invited: 'invited',
  member_role_changed: 'changed the role of',
  member_removed: 'removed',
  invite_withdrawn: 'withdrew an',
  team_created: 'created the team',
  team_updated: 'changed the team',
  team_deleted: 'deleted the team',
  team_member_set: 'made',
  team_member_removed: 'removed',
  credential_set: 'set the',
  credential_removed: 'removed the',
  guardrails_changed: 'changed the',
  label_deleted: 'deleted the label',
  knowledge_changed: 'changed the',
};

/** Actions whose subject is a thing of the workspace, not a name: it reads on in lower case. */
const GENERIC_SUBJECT = new Set<AuditAction>([
  'invite_withdrawn',
  'credential_set',
  'credential_removed',
  'guardrails_changed',
  'knowledge_changed',
]);

function Entry({ entry }: { entry: AuditEntry }) {
  const actor = entry.actor_name || 'Someone who left';
  const left = entry.action === 'member_removed' && entry.detail === 'left';
  return (
    <li className="flex items-center gap-2.5 px-3 py-2 text-[13px]">
      <PersonGlyph name={entry.actor_name || undefined} />
      <p className="min-w-0 flex-1">
        <span className="font-medium">{actor}</span>{' '}
        {left ? (
          <span className="text-muted-foreground">left the workspace</span>
        ) : (
          <>
            <span className="text-muted-foreground">{VERB[entry.action]}</span>{' '}
            <span className="break-words">
              {GENERIC_SUBJECT.has(entry.action) ? entry.subject.toLowerCase() : entry.subject}
            </span>
            {entry.detail && <span className="text-muted-foreground"> · {entry.detail}</span>}
          </>
        )}
      </p>
      <time
        dateTime={entry.created_at}
        title={formatDateTime(entry.created_at)}
        className="shrink-0 text-xs text-muted-foreground tabular-nums"
      >
        {formatRelative(entry.created_at)}
      </time>
    </li>
  );
}

function Log({ workspace }: { workspace: Workspace }) {
  const { data, error, isPending, hasNextPage, fetchNextPage, isFetchingNextPage } =
    useInfiniteQuery({
      queryKey: qk.audit(workspace.id),
      queryFn: ({ pageParam, signal }) => auditPage(workspace.id, pageParam, signal),
      initialPageParam: undefined as string | undefined,
      // A short page is the last one; otherwise continue below its oldest entry.
      getNextPageParam: (last) =>
        last.length < AUDIT_PAGE_SIZE ? undefined : last[last.length - 1]?.created_at,
    });
  if (isPending) {
    return (
      <div className="space-y-2" aria-busy>
        {[0, 1, 2, 3, 4].map((i) => (
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
  const entries = data.pages.flat();
  if (entries.length === 0) {
    return (
      <EmptyState
        icon={ScrollTextIcon}
        title="Nothing recorded yet"
        description="Changes to members, teams, the workspace credential, guardrails and labels are listed here from now on."
      />
    );
  }
  return (
    <div className="space-y-3">
      <ol className="divide-y rounded-lg border">
        {entries.map((entry) => (
          <Entry key={entry.id} entry={entry} />
        ))}
      </ol>
      {hasNextPage && (
        <Button
          variant="outline"
          size="sm"
          disabled={isFetchingNextPage}
          onClick={() => void fetchNextPage()}
        >
          Show older
        </Button>
      )}
    </div>
  );
}

function Audit() {
  const { current } = useCurrentWorkspace();
  if (!current) return <PageSkeleton />;
  const allowed = current.role === 'owner' || current.role === 'admin';
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <PageHeader
        title="Audit log"
        description={`Who changed the members, teams, credential and rules of ${current.name}, newest first.`}
      />
      {allowed ? (
        <Log key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={ScrollTextIcon}
          title="Only admins see the audit log"
          description="Ask an owner or admin of this workspace."
        />
      )}
    </div>
  );
}

export default function AuditPage() {
  return (
    <>
      <Seo title="Audit log" noIndex />
      <Audit />
    </>
  );
}
