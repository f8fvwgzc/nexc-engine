import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { GlassCard } from '@/components/custom-ui/glass-card';
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
import { ApiError, errorMessage } from '@/lib/api/errors';
import { useAuthStore } from '@/stores/auth-store';

import { deleteAccount, exportAccount } from '../api';

function DeleteDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate();
  const [password, setPassword] = useState('');
  const remove = useMutation({
    mutationFn: () => deleteAccount(password),
    meta: { errorToast: false },
    onSuccess: () => {
      useAuthStore.getState().clearSession();
      void navigate('/login', { replace: true });
    },
  });
  const close = () => {
    setPassword('');
    remove.reset();
    onClose();
  };
  const wrong = remove.error instanceof ApiError ? remove.error.fieldErrors.password : undefined;
  return (
    <Dialog open={open} onOpenChange={(next) => !next && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Delete your account?</DialogTitle>
          <DialogDescription>This cannot be undone.</DialogDescription>
        </DialogHeader>
        <ul className="list-disc space-y-1 pl-5 text-[13px] text-muted-foreground">
          <li>Workspaces you are alone in are deleted with everything in them.</li>
          <li>
            You leave every other workspace. What you made there stays, shown as written by a
            deleted account.
          </li>
          <li>
            Your name, address, password, sessions, AI account, notifications and personal memories
            are removed. Audit logs keep your name where it was already written.
          </li>
        </ul>
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (password) remove.mutate();
          }}
        >
          <div className="grid gap-1.5">
            <Label htmlFor="delete-password">Your password</Label>
            <Input
              id="delete-password"
              type="password"
              autoComplete="current-password"
              value={password}
              aria-invalid={Boolean(wrong)}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          {remove.error ? (
            <p role="alert" className="text-xs text-destructive">
              {wrong?.join(' ') ?? errorMessage(remove.error)}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={!password || remove.isPending}>
              Delete my account
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** A copy of what is held about the signed-in person, and deleting the account. */
export function DataCard() {
  const admin = useAuthStore((s) => s.user?.role === 'admin');
  const [deleting, setDeleting] = useState(false);
  const download = useMutation({
    mutationFn: exportAccount,
    meta: { successMessage: 'Your data was downloaded' },
  });
  return (
    <GlassCard id="your-data" className="scroll-mt-20 space-y-4 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Your data</h2>
        <p className="text-sm text-muted-foreground">
          A copy of what this installation holds about you: your account, the workspaces and teams
          you are in, the issues and comments you wrote, your graphs, documents and personal
          memories, and what you spent. It never contains a password or a key.
        </p>
      </div>
      <Button
        variant="outline"
        size="sm"
        disabled={download.isPending}
        onClick={() => download.mutate()}
      >
        {download.isPending ? 'Preparing…' : 'Download a copy'}
      </Button>
      <div className="space-y-2 border-t pt-4">
        <h3 className="text-sm font-medium">Delete account</h3>
        {admin ? (
          <p className="text-sm text-muted-foreground">
            This account administers the platform. Another administrator has to take that role away
            before it can be deleted.
          </p>
        ) : (
          <>
            <p className="text-sm text-muted-foreground">
              Removes what identifies you and every way in. If you are the only owner of a workspace
              other people work in, give it another owner or delete it first.
            </p>
            <Button variant="destructive" size="sm" onClick={() => setDeleting(true)}>
              Delete my account…
            </Button>
          </>
        )}
      </div>
      <DeleteDialog open={deleting} onClose={() => setDeleting(false)} />
    </GlassCard>
  );
}
