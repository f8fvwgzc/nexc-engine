import { useMutation } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { toast } from 'sonner';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ApiError, errorMessage } from '@/lib/api/errors';
import { PASSWORD_MIN_LENGTH } from '@/schemas/auth';
import { useAuthStore } from '@/stores/auth-store';

import { changePassword } from '../api';

/** Changes the password; the other devices are signed out, this one stays in. */
export function PasswordCard() {
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [again, setAgain] = useState('');
  const change = useMutation({
    mutationFn: () => changePassword({ current_password: current, new_password: next }),
    meta: { errorToast: false },
    onSuccess: (session) => {
      useAuthStore.getState().setSession(session);
      setCurrent('');
      setNext('');
      setAgain('');
      toast.success('Password changed. Every other device was signed out.');
    },
  });
  const fields = change.error instanceof ApiError ? change.error.fieldErrors : {};
  const mismatch = again.length > 0 && again !== next;
  const short = next.length > 0 && next.length < PASSWORD_MIN_LENGTH;
  const ready = current.length > 0 && next.length >= PASSWORD_MIN_LENGTH && again === next;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (ready) change.mutate();
  };
  return (
    <GlassCard id="password" className="scroll-mt-20 space-y-4 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Password</h2>
        <p className="text-sm text-muted-foreground">
          Changing it signs you out on every other device.
        </p>
      </div>
      <form onSubmit={submit} className="grid max-w-sm gap-3">
        <div className="grid gap-1.5">
          <Label htmlFor="current-password">Current password</Label>
          <Input
            id="current-password"
            type="password"
            autoComplete="current-password"
            value={current}
            aria-invalid={Boolean(fields.current_password)}
            onChange={(e) => setCurrent(e.target.value)}
          />
          {fields.current_password && (
            <p role="alert" className="text-xs text-destructive">
              {fields.current_password.join(' ')}
            </p>
          )}
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor="new-password">New password</Label>
          <Input
            id="new-password"
            type="password"
            autoComplete="new-password"
            value={next}
            aria-invalid={short || Boolean(fields.new_password)}
            onChange={(e) => setNext(e.target.value)}
          />
          <p className={`text-xs ${short ? 'text-destructive' : 'text-muted-foreground'}`}>
            {fields.new_password?.join(' ') ?? `At least ${PASSWORD_MIN_LENGTH} characters.`}
          </p>
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor="repeat-password">New password again</Label>
          <Input
            id="repeat-password"
            type="password"
            autoComplete="new-password"
            value={again}
            aria-invalid={mismatch}
            onChange={(e) => setAgain(e.target.value)}
          />
          {mismatch && <p className="text-xs text-destructive">The two do not match.</p>}
        </div>
        {change.error && Object.keys(fields).length === 0 ? (
          <p role="alert" className="text-xs text-destructive">
            {errorMessage(change.error)}
          </p>
        ) : null}
        <div>
          <Button type="submit" size="sm" disabled={!ready || change.isPending}>
            {change.isPending ? 'Changing…' : 'Change password'}
          </Button>
        </div>
      </form>
    </GlassCard>
  );
}
