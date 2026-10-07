import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { MoreHorizontalIcon } from 'lucide-react';
import { useState } from 'react';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { PersonGlyph } from '@/components/custom-ui/issue-glyphs';
import { PageHeader } from '@/components/custom-ui/page-header';
import { Seo } from '@/components/seo/seo';
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { Textarea } from '@/components/ui/textarea';
import {
  eraseAccount,
  issuePasswordReset,
  platformUsersQuery,
  updatePlatformUser,
  type PlatformUser,
  type PlatformUserChange,
} from '@/features/platform/api';
import { PLATFORM_PAGE_SIZE, usePlatformPage } from '@/features/platform/paged-list';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { useAuthStore } from '@/stores/auth-store';

const REASON_MAX = 300;

/** Asks why, then suspends: the reason goes to the activity log. */
function SuspendDialog({
  user,
  pending,
  error,
  onCancel,
  onConfirm,
}: {
  user: PlatformUser | null;
  pending: boolean;
  error: unknown;
  onCancel: () => void;
  onConfirm: (reason: string) => void;
}) {
  const [reason, setReason] = useState('');
  const close = () => {
    setReason('');
    onCancel();
  };
  return (
    <Dialog open={user !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Suspend {user?.name}?</DialogTitle>
          <DialogDescription>
            {user?.email} is signed out everywhere at once and cannot sign in until the account is
            reactivated. Nothing is deleted: their workspaces and what they made stay as they are.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            onConfirm(reason.trim());
            setReason('');
          }}
        >
          <div className="space-y-1.5">
            <Label htmlFor="suspend-reason" className="text-[13px]">
              Reason
            </Label>
            <Textarea
              id="suspend-reason"
              value={reason}
              maxLength={REASON_MAX}
              rows={3}
              placeholder="Kept in the platform's activity log. The account holder does not see it."
              className="text-[13px]"
              onChange={(e) => setReason(e.target.value)}
            />
          </div>
          {error ? (
            <p role="alert" className="text-xs text-destructive">
              {errorMessage(error)}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={pending}>
              Suspend account
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Issues a one-time password reset link for someone who cannot sign in, and shows it once. The
 * installation sends no e-mail: the administrator hands the link over.
 */
function ResetLinkDialog({ user, onClose }: { user: PlatformUser | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const issue = useMutation({
    mutationFn: (target: PlatformUser) => issuePasswordReset(target.id),
    meta: { errorToast: false },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['platform', 'events'] }),
  });
  const close = () => {
    issue.reset();
    onClose();
  };
  // The token is after `#`: a browser does not send that part to a server.
  const link = issue.data ? `${window.location.origin}/reset-password#${issue.data.token}` : null;
  return (
    <Dialog open={user !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Password reset link for {user?.name}</DialogTitle>
          <DialogDescription>
            For when {user?.email} cannot sign in. The link works once and for one hour, and anyone
            who has it can set the account&apos;s password: hand it over through a channel you
            trust. Creating a new link cancels the one before.
          </DialogDescription>
        </DialogHeader>
        {link ? (
          <div className="space-y-2">
            <div className="flex items-center gap-1">
              <Input
                readOnly
                value={link}
                aria-label="Reset link"
                className="font-mono text-xs"
                onFocus={(e) => e.target.select()}
              />
              <CopyButton value={link} label="Copy reset link" />
            </div>
            <p className="text-xs text-muted-foreground">
              It is shown this once and is not kept anywhere it can be read again.
            </p>
          </div>
        ) : null}
        {issue.error ? (
          <p role="alert" className="text-xs text-destructive">
            {errorMessage(issue.error)}
          </p>
        ) : null}
        <DialogFooter>
          <Button type="button" variant="outline" onClick={close}>
            {link ? 'Done' : 'Cancel'}
          </Button>
          {!link && (
            <Button disabled={issue.isPending} onClick={() => user && issue.mutate(user)}>
              Create link
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Erases an account on its holder's request, once its address is typed out. */
function EraseDialog({ user, onClose }: { user: PlatformUser | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [typed, setTyped] = useState('');
  const erase = useMutation({
    mutationFn: (target: PlatformUser) => eraseAccount(target.id, typed.trim()),
    meta: { successMessage: 'Account erased', errorToast: false },
    onSuccess: () => {
      setTyped('');
      onClose();
      return queryClient.invalidateQueries({ queryKey: ['platform'] });
    },
  });
  const close = () => {
    setTyped('');
    erase.reset();
    onClose();
  };
  const matches = user !== null && typed.trim().toLowerCase() === user.email.toLowerCase();
  return (
    <Dialog open={user !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Erase {user?.name}&apos;s account?</DialogTitle>
          <DialogDescription>
            For when the account&apos;s holder asks for it. Workspaces they are alone in are
            deleted; they leave the others, where what they made stays under &quot;Deleted
            account&quot;. Their name, address, password and sessions are removed. This cannot be
            undone, and the activity log keeps only the account&apos;s id.
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (user && matches) erase.mutate(user);
          }}
        >
          <div className="space-y-1.5">
            <Label htmlFor="erase-confirm" className="text-[13px]">
              Type {user?.email} to confirm
            </Label>
            <Input
              id="erase-confirm"
              value={typed}
              autoComplete="off"
              className="text-[13px]"
              onChange={(e) => setTyped(e.target.value)}
            />
          </div>
          {erase.error ? (
            <p role="alert" className="text-xs text-destructive">
              {errorMessage(erase.error)}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={!matches || erase.isPending}>
              Erase account
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export default function PlatformUsersPage() {
  const queryClient = useQueryClient();
  const me = useAuthStore((s) => s.user?.id);
  const { page, search, controls } = usePlatformPage();
  const { data, isPending, error } = useQuery(platformUsersQuery(page));
  const [suspending, setSuspending] = useState<PlatformUser | null>(null);
  const [erasing, setErasing] = useState<PlatformUser | null>(null);
  const [resetting, setResetting] = useState<PlatformUser | null>(null);
  const change = useMutation({
    mutationFn: ({ user, change }: { user: PlatformUser; change: PlatformUserChange }) =>
      updatePlatformUser(user.id, change),
    meta: { successMessage: 'Account updated' },
    onSuccess: () => {
      setSuspending(null);
      return queryClient.invalidateQueries({ queryKey: ['platform'] });
    },
  });
  const rows = data?.slice(0, PLATFORM_PAGE_SIZE) ?? [];
  return (
    <div className="mx-auto w-full max-w-5xl space-y-5 p-4 sm:p-6">
      <Seo title="Accounts" noIndex />
      <PageHeader
        title="Accounts"
        description="Everyone who registered. A platform administrator works in this console only and never inside a workspace; what anyone else may do inside a workspace is set in that workspace, not here."
      />
      {search}
      {change.error && !suspending ? (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(change.error)}
        </p>
      ) : null}
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
              {u.suspended && (
                <Badge variant="destructive" title={u.suspended_reason || undefined}>
                  Suspended
                </Badge>
              )}
              {u.locked && <Badge variant="outline">Locked</Badge>}
              {u.two_factor && <Badge variant="outline">2FA</Badge>}
              <Badge
                variant={u.role === 'admin' ? 'default' : 'outline'}
                className="w-28 justify-center"
              >
                {u.role === 'admin' ? 'Platform admin' : 'User'}
              </Badge>
              <span className="hidden w-24 text-right text-xs text-muted-foreground sm:block">
                {formatRelative(u.created_at)}
              </span>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Actions for ${u.name}`}
                    disabled={u.id === me || change.isPending}
                    title={u.id === me ? 'Nobody changes their own account here' : undefined}
                  >
                    <MoreHorizontalIcon />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="min-w-52">
                  <DropdownMenuItem
                    onSelect={() =>
                      change.mutate({
                        user: u,
                        change: { role: u.role === 'admin' ? 'user' : 'admin' },
                      })
                    }
                  >
                    {u.role === 'admin' ? 'Remove platform admin' : 'Make platform admin'}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => setResetting(u)}>
                    Password reset link…
                  </DropdownMenuItem>
                  {u.two_factor && (
                    <DropdownMenuItem
                      title="For someone who lost their authenticator app and their recovery codes"
                      onSelect={() => change.mutate({ user: u, change: { two_factor: false } })}
                    >
                      Turn off two-factor
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuSeparator />
                  {u.suspended ? (
                    <DropdownMenuItem
                      onSelect={() => change.mutate({ user: u, change: { suspended: false } })}
                    >
                      Reactivate account
                    </DropdownMenuItem>
                  ) : (
                    <DropdownMenuItem
                      variant="destructive"
                      onSelect={() => {
                        change.reset();
                        setSuspending(u);
                      }}
                    >
                      Suspend account…
                    </DropdownMenuItem>
                  )}
                  {u.role !== 'admin' && (
                    <DropdownMenuItem variant="destructive" onSelect={() => setErasing(u)}>
                      Erase account…
                    </DropdownMenuItem>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </li>
          ))}
        </ul>
      )}
      {controls(data?.length ?? 0)}
      <ResetLinkDialog user={resetting} onClose={() => setResetting(null)} />
      <EraseDialog user={erasing} onClose={() => setErasing(null)} />
      <SuspendDialog
        user={suspending}
        pending={change.isPending}
        error={suspending ? change.error : null}
        onCancel={() => setSuspending(null)}
        onConfirm={(reason) =>
          suspending && change.mutate({ user: suspending, change: { suspended: true, reason } })
        }
      />
    </div>
  );
}
