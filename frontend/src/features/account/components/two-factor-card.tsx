import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { GlassCard } from '@/components/custom-ui/glass-card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ApiError, errorMessage } from '@/lib/api/errors';

import { disableTwoFactor, enableTwoFactor, setupTwoFactor, twoFactorQuery } from '../api';

function problem(error: unknown): string | null {
  if (!error) return null;
  if (error instanceof ApiError) {
    const fields = Object.values(error.fieldErrors).flat();
    if (fields.length > 0) return fields.join(' ');
  }
  return errorMessage(error);
}

/** Setting up: the secret for the app, then the app's first code, then the recovery codes. */
function Setup({ onDone }: { onDone: () => void }) {
  const queryClient = useQueryClient();
  const [code, setCode] = useState('');
  const setup = useMutation({ mutationFn: setupTwoFactor, meta: { errorToast: false } });
  const enable = useMutation({
    mutationFn: () => enableTwoFactor(code.trim()),
    meta: { errorToast: false },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['account'] }),
  });
  if (enable.data) {
    const codes = enable.data.recovery_codes.join('\n');
    return (
      <div className="space-y-3">
        <p className="text-sm">
          Two-factor sign-in is on. Keep these recovery codes somewhere safe: each signs you in once
          if you lose the app, and they are not shown again.
        </p>
        <pre className="max-w-sm rounded-md border bg-muted/40 p-3 font-mono text-[13px] leading-6">
          {codes}
        </pre>
        <div className="flex items-center gap-2">
          <CopyButton value={codes} label="Copy recovery codes" />
          <Button size="sm" onClick={onDone}>
            I have saved them
          </Button>
        </div>
      </div>
    );
  }
  if (!setup.data) {
    return (
      <div className="space-y-2">
        <Button size="sm" disabled={setup.isPending} onClick={() => setup.mutate()}>
          Set up two-factor sign-in
        </Button>
        {setup.error ? (
          <p role="alert" className="text-xs text-destructive">
            {problem(setup.error)}
          </p>
        ) : null}
      </div>
    );
  }
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (code.trim()) enable.mutate();
  };
  return (
    <form onSubmit={submit} className="grid max-w-md gap-3">
      <p className="text-sm text-muted-foreground">
        1. Add an account in your authenticator app:{' '}
        <a
          href={setup.data.uri}
          className="font-medium text-foreground underline underline-offset-4"
        >
          open it on this device
        </a>
        , or type this key into the app.
      </p>
      <div className="flex items-center gap-1">
        <Input
          readOnly
          value={setup.data.secret}
          aria-label="Setup key"
          className="font-mono text-xs"
        />
        <CopyButton value={setup.data.secret} label="Copy setup key" />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="two-factor-code">2. Enter the 6-digit code the app shows</Label>
        <Input
          id="two-factor-code"
          inputMode="numeric"
          autoComplete="one-time-code"
          value={code}
          className="w-40"
          onChange={(e) => setCode(e.target.value)}
        />
      </div>
      {enable.error ? (
        <p role="alert" className="text-xs text-destructive">
          {problem(enable.error)}
        </p>
      ) : null}
      <div>
        <Button type="submit" size="sm" disabled={!code.trim() || enable.isPending}>
          Turn on
        </Button>
      </div>
    </form>
  );
}

function TurnOff() {
  const queryClient = useQueryClient();
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const disable = useMutation({
    mutationFn: () => disableTwoFactor(password, code.trim()),
    meta: { errorToast: false, successMessage: 'Two-factor sign-in turned off' },
    onSuccess: () => {
      setPassword('');
      setCode('');
      return queryClient.invalidateQueries({ queryKey: ['account'] });
    },
  });
  return (
    <form
      className="grid max-w-sm gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (password && code.trim()) disable.mutate();
      }}
    >
      <div className="grid gap-1.5">
        <Label htmlFor="two-factor-password">Your password</Label>
        <Input
          id="two-factor-password"
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor="two-factor-off-code">A code from the app, or a recovery code</Label>
        <Input
          id="two-factor-off-code"
          autoComplete="one-time-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
        />
      </div>
      {disable.error ? (
        <p role="alert" className="text-xs text-destructive">
          {problem(disable.error)}
        </p>
      ) : null}
      <div>
        <Button
          type="submit"
          variant="outline"
          size="sm"
          disabled={!password || !code.trim() || disable.isPending}
        >
          Turn off two-factor sign-in
        </Button>
      </div>
    </form>
  );
}

/** A second step at sign-in: a code from an authenticator app after the password. */
export function TwoFactorCard() {
  const { data } = useQuery(twoFactorQuery());
  // The recovery codes stay on screen after the factor is on, until they were saved.
  const [settingUp, setSettingUp] = useState(false);
  const showSetup = settingUp || data?.enabled === false;
  return (
    <GlassCard id="two-factor" className="scroll-mt-20 space-y-4 p-5 sm:p-6">
      <div className="flex items-start justify-between gap-2">
        <div>
          <h2 className="font-medium">Two-factor sign-in</h2>
          <p className="text-sm text-muted-foreground">
            After your password, signing in asks for a code from an authenticator app on your phone.
            Someone who learns your password still cannot get in.
          </p>
        </div>
        {data && (
          <Badge variant={data.enabled ? 'default' : 'outline'}>
            {data.enabled ? 'On' : 'Off'}
          </Badge>
        )}
      </div>
      {!data ? null : showSetup ? (
        <div onFocus={() => setSettingUp(true)} onClick={() => setSettingUp(true)}>
          <Setup onDone={() => setSettingUp(false)} />
        </div>
      ) : (
        <div className="space-y-3">
          <p className="text-sm text-muted-foreground">
            {data.recovery_codes_left} of your recovery codes are unused.
          </p>
          <TurnOff />
        </div>
      )}
    </GlassCard>
  );
}
