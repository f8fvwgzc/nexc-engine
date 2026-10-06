import { useMutation } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link, useLocation } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { resetPassword } from '@/features/auth/api';
import { AuthCard } from '@/features/auth/components/auth-card';
import { ApiError, errorMessage } from '@/lib/api/errors';
import { PASSWORD_MIN_LENGTH } from '@/schemas/auth';

const signIn = (
  <Link to="/login" className="font-medium text-foreground underline-offset-4 hover:underline">
    Sign in
  </Link>
);

/**
 * Where a password reset link leads. The token is the part of the address after `#`, which the
 * browser never sends to a server, so it stays out of access logs.
 */
export default function ResetPasswordPage() {
  const token = useLocation().hash.replace(/^#/, '');
  const [next, setNext] = useState('');
  const [again, setAgain] = useState('');
  const reset = useMutation({
    mutationFn: () => resetPassword(token, next),
    meta: { errorToast: false },
  });
  const short = next.length > 0 && next.length < PASSWORD_MIN_LENGTH;
  const mismatch = again.length > 0 && again !== next;
  const ready = next.length >= PASSWORD_MIN_LENGTH && again === next;
  const fields = reset.error instanceof ApiError ? reset.error.fieldErrors : {};
  const spent = reset.error instanceof ApiError && reset.error.status === 401;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (ready) reset.mutate();
  };
  return (
    <>
      <Seo title="Set a new password" noIndex />
      <AuthCard
        title="Set a new password"
        description={
          reset.isSuccess
            ? 'Your password is set.'
            : 'Choose a password for your account. The link you opened works once.'
        }
        footer={<>Remembered it after all? {signIn}</>}
      >
        {reset.isSuccess ? (
          <p className="text-center text-sm">
            You were signed out everywhere. {signIn} with your new password.
          </p>
        ) : !token ? (
          <p role="alert" className="text-center text-sm text-destructive">
            This address has no reset link in it. Open the link exactly as you were given it.
          </p>
        ) : (
          <form noValidate onSubmit={submit} className="grid gap-4">
            <div className="grid gap-1.5">
              <Label htmlFor="reset-new">New password</Label>
              <Input
                id="reset-new"
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
              <Label htmlFor="reset-again">New password again</Label>
              <Input
                id="reset-again"
                type="password"
                autoComplete="new-password"
                value={again}
                aria-invalid={mismatch}
                onChange={(e) => setAgain(e.target.value)}
              />
              {mismatch && <p className="text-xs text-destructive">The two do not match.</p>}
            </div>
            {reset.error && Object.keys(fields).length === 0 ? (
              <p role="alert" className="text-sm text-destructive">
                {spent
                  ? 'This link does not work any more: it was used, replaced or has expired. Ask for a new one.'
                  : errorMessage(reset.error)}
              </p>
            ) : null}
            <Button type="submit" size="lg" className="w-full" disabled={!ready || reset.isPending}>
              {reset.isPending ? 'Setting…' : 'Set password'}
            </Button>
          </form>
        )}
      </AuthCard>
    </>
  );
}
