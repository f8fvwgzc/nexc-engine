import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { DatabaseIcon } from 'lucide-react';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { z } from 'zod';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { EmptyState } from '@/components/custom-ui/empty-state';
import { PageHeader } from '@/components/custom-ui/page-header';
import { PageSkeleton } from '@/components/layout/page-skeleton';
import { Seo } from '@/components/seo/seo';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { apiRequest, apiSend } from '@/lib/api/client';
import { errorMessage } from '@/lib/api/errors';
import { formatInteger, formatRelative } from '@/lib/format';
import { qk } from '@/lib/query-keys';
import { idSchema, timestampSchema } from '@/schemas/common';
import type { Workspace } from '@/schemas/workspace';

const transferSchema = z.object({
  id: idSchema,
  target: z.string(),
  status: z.enum(['running', 'done', 'failed']),
  report: z.array(z.object({ table: z.string(), read: z.number(), written: z.number() })),
  error: z.string(),
  created_at: timestampSchema,
  finished_at: timestampSchema.nullable(),
});
type Transfer = z.infer<typeof transferSchema>;

// Outside the roots copied to browser storage.
const key = (workspaceId: string) => ['transfers', workspaceId] as const;

function Report({ transfer }: { transfer: Transfer }) {
  const rows = transfer.report.filter((t) => t.read > 0);
  const total = rows.reduce((sum, t) => sum + t.written, 0);
  return (
    <li className="space-y-2 px-3 py-2.5 text-[13px]">
      <p className="flex flex-wrap items-center gap-2">
        <Badge
          variant={
            transfer.status === 'failed'
              ? 'destructive'
              : transfer.status === 'done'
                ? 'outline'
                : 'secondary'
          }
          className={transfer.status === 'running' ? 'motion-safe:animate-pulse' : ''}
        >
          {transfer.status === 'done'
            ? 'Copied'
            : transfer.status === 'failed'
              ? 'Failed'
              : 'Copying'}
        </Badge>
        <span className="font-mono text-xs break-all">{transfer.target}</span>
        <span className="ml-auto text-xs text-muted-foreground">
          {formatRelative(transfer.created_at)}
        </span>
      </p>
      {transfer.error && <p className="text-destructive">{transfer.error}</p>}
      {transfer.status === 'done' && (
        <details>
          <summary className="cursor-pointer text-muted-foreground">
            {formatInteger(total)} rows in {rows.length} tables
          </summary>
          <ul className="mt-1 grid grid-cols-2 gap-x-6 text-xs text-muted-foreground sm:grid-cols-3">
            {rows.map((t) => (
              <li key={t.table} className="flex justify-between gap-2">
                <span>{t.table}</span>
                <span className="tabular-nums">
                  {formatInteger(t.written)}
                  {t.written !== t.read ? ` of ${formatInteger(t.read)}` : ''}
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </li>
  );
}

function TransferForm({ workspace }: { workspace: Workspace }) {
  const queryClient = useQueryClient();
  const [url, setUrl] = useState('');
  const { data: transfers = [] } = useQuery({
    queryKey: key(workspace.id),
    queryFn: ({ signal }) =>
      apiRequest(`/workspaces/${workspace.id}/transfers`, z.array(transferSchema), { signal }),
    // While a copy runs, look again every two seconds.
    refetchInterval: (query) =>
      query.state.data?.some((t) => t.status === 'running') ? 2_000 : false,
  });
  const start = useMutation({
    mutationFn: () =>
      apiRequest(`/workspaces/${workspace.id}/transfers`, transferSchema, {
        method: 'POST',
        body: { url: url.trim() },
      }),
    meta: { errorToast: false },
    onSuccess: () => {
      setUrl('');
      void queryClient.invalidateQueries({ queryKey: key(workspace.id) });
    },
  });
  const copied = transfers.some((t) => t.status === 'done');
  const navigate = useNavigate();
  const [removing, setRemoving] = useState(false);
  const remove = useMutation({
    mutationFn: () => apiSend(`/workspaces/${workspace.id}`, { method: 'DELETE' }),
    meta: { errorToast: false, successMessage: 'Workspace removed from this server' },
    onSuccess: () => {
      // Everything cached belongs to a workspace that is gone.
      queryClient.clear();
      void queryClient.invalidateQueries({ queryKey: qk.workspaces.all });
      void navigate('/app');
    },
  });
  return (
    <div className="space-y-6">
      <section className="space-y-3 rounded-lg border p-4">
        <div className="space-y-1 text-[13px] text-muted-foreground">
          <h2 className="font-medium text-foreground">Copy this workspace to your PostgreSQL</h2>
          <p>
            Paste the connection string of an empty database you control. Its tables are created
            there, then everything of {workspace.name} is copied: members, teams, issues, projects,
            graphs, runs, documents’ passages, memory, settings and logs.
          </p>
          <p>
            Not copied: password hashes (members sign up again or reset on your server), stored API
            keys (enter them again), and files on disk (uploaded originals, run artifacts). The
            connection string is used once and not kept. Nothing changes here.
          </p>
        </div>
        <form
          className="flex flex-wrap gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (url.trim()) start.mutate();
          }}
        >
          <Input
            type="password"
            value={url}
            autoComplete="off"
            placeholder="postgres://user:password@host:5432/database"
            aria-label="Target database URL"
            className="h-8 min-w-64 flex-1 font-mono text-xs"
            onChange={(e) => setUrl(e.target.value)}
          />
          <Button type="submit" size="sm" disabled={!url.trim() || start.isPending}>
            Copy workspace
          </Button>
        </form>
        {start.error && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(start.error)}
          </p>
        )}
      </section>
      {transfers.length > 0 && (
        <section aria-label="Transfers" className="space-y-2">
          <h2 className="text-[13px] font-medium">Transfers</h2>
          <ul className="divide-y rounded-lg border">
            {transfers.map((t) => (
              <Report key={t.id} transfer={t} />
            ))}
          </ul>
        </section>
      )}
      <section className="space-y-2 rounded-lg border border-dashed p-4 text-[13px] text-muted-foreground">
        <h2 className="font-medium text-foreground">Then remove it from this server</h2>
        <p>
          {copied
            ? 'A copy succeeded. Check it on your side before removing anything here.'
            : 'Copy the workspace first and check the copy; removal is offered after a copy succeeded.'}{' '}
          Removing deletes every row of {workspace.name} from this database and cannot be undone.
          Backups and logs of this server, and files on its disk, are outside what it reaches.
        </p>
        <Button
          variant="destructive"
          size="sm"
          disabled={!copied || remove.isPending}
          onClick={() => setRemoving(true)}
        >
          Remove {workspace.name} from this server
        </Button>
        {remove.error && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(remove.error)}
          </p>
        )}
        <ConfirmDialog
          open={removing}
          onOpenChange={setRemoving}
          title={`Remove ${workspace.name} from this server?`}
          description="Every issue, graph, run, document passage, memory and setting of this workspace is deleted here for all its members. This cannot be undone."
          confirmLabel="Remove workspace"
          destructive
          onConfirm={() => remove.mutate()}
        />
      </section>
    </div>
  );
}

export default function TransferPage() {
  const { current } = useCurrentWorkspace();
  return (
    <div className="mx-auto w-full max-w-4xl space-y-6 p-4 sm:p-6">
      <Seo title="Data transfer" noIndex />
      <PageHeader
        title="Data transfer"
        description="Hold your workspace’s data yourself: copy it to a PostgreSQL you control, then remove it from this server."
      />
      {!current ? (
        <PageSkeleton />
      ) : current.role === 'owner' ? (
        <TransferForm key={current.id} workspace={current} />
      ) : (
        <EmptyState
          icon={DatabaseIcon}
          title="Only the workspace’s owner transfers its data"
          description="It copies everything the workspace holds."
        />
      )}
    </div>
  );
}
