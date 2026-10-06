import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';

import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet';
import { Skeleton } from '@/components/ui/skeleton';
import { errorMessage } from '@/lib/api/errors';
import { formatBytes, formatDateTime, formatInteger } from '@/lib/format';

import {
  assignOwner,
  deletePlatformWorkspace,
  platformWorkspaceQuery,
  type PlatformMember,
  type PlatformWorkspaceDetail,
} from './api';

/** Whether a member can open the workspace at all. */
function usable(member: PlatformMember): boolean {
  return !member.suspended && !member.platform_admin;
}

function Count({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border px-2.5 py-2">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function Members({ members }: { members: PlatformMember[] }) {
  return (
    <ul className="divide-y overflow-hidden rounded-lg border">
      {members.map((m) => (
        <li
          key={m.user_id}
          className="flex min-h-10 items-center gap-2.5 px-2.5 py-1.5 text-[13px]"
        >
          <PersonGlyph name={m.name} />
          <span className="min-w-0 flex-1">
            <span className="block truncate font-medium">{m.name}</span>
            <span className="block truncate text-xs text-muted-foreground">{m.email}</span>
          </span>
          {m.suspended && <Badge variant="destructive">Suspended</Badge>}
          {m.platform_admin && <Badge variant="secondary">Platform admin</Badge>}
          <Badge variant={m.role === 'owner' ? 'default' : 'outline'} className="capitalize">
            {m.role}
          </Badge>
        </li>
      ))}
    </ul>
  );
}

function AssignOwner({ workspace }: { workspace: PlatformWorkspaceDetail }) {
  const queryClient = useQueryClient();
  const [email, setEmail] = useState('');
  const assign = useMutation({
    mutationFn: () => assignOwner(workspace.id, email.trim()),
    meta: { successMessage: 'Owner assigned' },
    onSuccess: (detail) => {
      setEmail('');
      queryClient.setQueryData(platformWorkspaceQuery(workspace.id).queryKey, detail);
      void queryClient.invalidateQueries({ queryKey: ['platform'] });
    },
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (email.trim()) assign.mutate();
  };
  return (
    <form onSubmit={submit} className="space-y-1.5">
      <Label htmlFor="assign-owner" className="text-[13px]">
        Assign an owner
      </Label>
      <div className="flex gap-2">
        <Input
          id="assign-owner"
          type="email"
          value={email}
          placeholder="E-mail of a registered account"
          className="h-8 text-[13px]"
          onChange={(e) => setEmail(e.target.value)}
        />
        <Button type="submit" size="sm" disabled={!email.trim() || assign.isPending}>
          Assign
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        For a workspace whose owner left or cannot sign in. The account becomes an owner next to the
        ones there are, and the workspace&apos;s own audit log shows that the platform did it.
      </p>
      {assign.error && (
        <p role="alert" className="text-xs text-destructive">
          {errorMessage(assign.error)}
        </p>
      )}
    </form>
  );
}

function DeleteWorkspace({
  workspace,
  onDeleted,
}: {
  workspace: PlatformWorkspaceDetail;
  onDeleted: () => void;
}) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState('');
  const remove = useMutation({
    mutationFn: () => deletePlatformWorkspace(workspace.id, typed),
    meta: { successMessage: 'Workspace deleted' },
    onSuccess: () => {
      setOpen(false);
      onDeleted();
      void queryClient.invalidateQueries({ queryKey: ['platform'] });
    },
  });
  const close = (next: boolean) => {
    setOpen(next);
    if (!next) {
      setTyped('');
      remove.reset();
    }
  };
  return (
    <div className="space-y-1.5 rounded-lg border border-destructive/30 p-3">
      <p className="text-[13px] font-medium">Delete this workspace</p>
      <p className="text-xs text-muted-foreground">
        Removes its issues, graphs, runs, documents and memories for all {workspace.member_count}{' '}
        {workspace.member_count === 1 ? 'member' : 'members'}. An owner can copy the data to their
        own database or download its files first, in the workspace under Settings, Data.
      </p>
      <Button variant="destructive" size="sm" onClick={() => setOpen(true)}>
        Delete workspace…
      </Button>
      <Dialog open={open} onOpenChange={close}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Delete {workspace.name}?</DialogTitle>
            <DialogDescription>
              Everything in it is removed and cannot be brought back. Members who have no other
              workspace get an empty personal one. Type the workspace&apos;s name to confirm.
            </DialogDescription>
          </DialogHeader>
          <form
            className="space-y-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (typed === workspace.name) remove.mutate();
            }}
          >
            <Input
              value={typed}
              aria-label="Workspace name"
              placeholder={workspace.name}
              autoComplete="off"
              onChange={(e) => setTyped(e.target.value)}
            />
            {remove.error && (
              <p role="alert" className="text-xs text-destructive">
                {errorMessage(remove.error)}
              </p>
            )}
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => close(false)}>
                Cancel
              </Button>
              <Button
                type="submit"
                variant="destructive"
                disabled={typed !== workspace.name || remove.isPending}
              >
                Delete forever
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Detail({
  workspace,
  onDeleted,
}: {
  workspace: PlatformWorkspaceDetail;
  onDeleted: () => void;
}) {
  const { footprint } = workspace;
  const owned = workspace.members.some((m) => m.role === 'owner' && usable(m));
  return (
    <div className="space-y-5 px-4 pb-6">
      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Count label="Members" value={formatInteger(workspace.member_count)} />
        <Count label="Teams" value={formatInteger(workspace.team_count)} />
        <Count label="Projects" value={formatInteger(footprint.project_count)} />
        <Count label="Issues" value={formatInteger(workspace.issue_count)} />
        <Count label="Graphs" value={formatInteger(workspace.graph_count)} />
        <Count label="Runs" value={formatInteger(footprint.run_count)} />
        <Count label="Memories" value={formatInteger(footprint.memory_count)} />
        <Count
          label="Documents"
          value={`${formatInteger(footprint.document_count)} · ${formatBytes(footprint.document_bytes)}`}
        />
      </dl>
      <section className="space-y-2">
        <h3 className="text-[13px] font-medium">Members</h3>
        {!owned && (
          <p role="status" className="rounded-md bg-destructive/10 px-2.5 py-2 text-xs">
            No owner of this workspace can sign in to it. Assign one below.
          </p>
        )}
        <Members members={workspace.members} />
      </section>
      <AssignOwner workspace={workspace} />
      <DeleteWorkspace workspace={workspace} onDeleted={onDeleted} />
    </div>
  );
}

/**
 * One workspace as the platform sees it: who is in it and how much it holds, with what the
 * platform may do to it. Nothing of its content is fetched or shown.
 */
export function WorkspaceSheet({ id, onClose }: { id: string | null; onClose: () => void }) {
  const { data, error, isPending } = useQuery({
    ...platformWorkspaceQuery(id ?? ''),
    enabled: id !== null,
  });
  return (
    <Sheet open={id !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="gap-2 overflow-y-auto data-[side=right]:w-full data-[side=right]:sm:max-w-xl">
        <SheetHeader>
          <SheetTitle>{data?.name ?? 'Workspace'}</SheetTitle>
          <SheetDescription>
            {data
              ? `Created ${formatDateTime(data.created_at)}. Its content is not shown here.`
              : 'Who is in it and how much it holds.'}
          </SheetDescription>
        </SheetHeader>
        {error ? (
          <p role="alert" className="px-4 text-sm text-destructive">
            {errorMessage(error)}
          </p>
        ) : isPending || !data ? (
          <div className="space-y-2 px-4" aria-busy>
            <Skeleton className="h-24 w-full" />
            <Skeleton className="h-40 w-full" />
          </div>
        ) : (
          <Detail workspace={data} onDeleted={onClose} />
        )}
      </SheetContent>
    </Sheet>
  );
}
