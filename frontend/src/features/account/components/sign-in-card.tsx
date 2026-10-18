import { useMutation, useQuery } from '@tanstack/react-query';
import { KeyRoundIcon } from 'lucide-react';
import { useState, type FormEvent } from 'react';
import { useSearchParams } from 'react-router-dom';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { providersQuery } from '@/features/auth/api';
import { ssoErrorMessage } from '@/features/auth/sso-errors';
import { ApiError, errorMessage } from '@/lib/api/errors';
import { formatDateTime, formatRelative } from '@/lib/format';

import { linkIdentity, signInMethodsQuery } from '../api';

/**
 * The single sign-on identities the account is linked to, and whether it also has a password.
 * While the installation has a provider, an account with a password and no identity of it links
 * one here (its password confirms): the browser goes to the provider and comes back to this page,
 * with `sso_error` when the link did not happen. Nothing shows when nothing is linked and there is
 * nothing to link.
 */
export function SignInCard() {
  const { data } = useQuery(signInMethodsQuery());
  const { data: providers } = useQuery(providersQuery());
  const [params] = useSearchParams();
  const sso = providers?.sso[0];
  if (!data) return null;
  const canLink = sso !== undefined && data.password && !data.identities.some((i) => i.current);
  if (data.identities.length === 0 && !canLink) return null;
  const ssoError = params.get('sso_error');
  return (
    <GlassCard id="single-sign-on" className="scroll-mt-20 space-y-3 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Single sign-on</h2>
        <p className="text-sm text-muted-foreground">
          {data.identities.length === 0
            ? 'You sign in with your password.'
            : data.password
              ? 'You sign in through your organisation’s identity provider, or with your password.'
              : 'You sign in through your organisation’s identity provider; this account has no password here.'}
        </p>
      </div>
      {ssoError ? (
        <p role="alert" className="text-sm text-destructive">
          {ssoErrorMessage(ssoError, sso?.display_name ?? 'single sign-on')}
        </p>
      ) : null}
      {data.identities.length > 0 ? (
        <ul className="divide-y rounded-lg border text-[13px]">
          {data.identities.map((identity) => (
            <li
              key={identity.issuer}
              className="flex flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-2"
            >
              <KeyRoundIcon className="size-3.5 text-muted-foreground" aria-hidden />
              <span className="font-medium">{identity.provider}</span>
              <span className="text-muted-foreground">{identity.email}</span>
              <span
                className="ml-auto shrink-0 text-xs text-muted-foreground"
                title={`Linked ${formatDateTime(identity.linked_at)}`}
              >
                {identity.last_login_at
                  ? `last sign-in ${formatRelative(identity.last_login_at)}`
                  : `linked ${formatRelative(identity.linked_at)}`}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
      {canLink ? <LinkForm provider={sso.display_name} /> : null}
    </GlassCard>
  );
}

/** Links the provider's identity: the password confirms, then the browser goes to the provider. */
function LinkForm({ provider }: { provider: string }) {
  const [password, setPassword] = useState('');
  const start = useMutation({
    mutationFn: () => linkIdentity(password),
    meta: { errorToast: false },
    onSuccess: ({ authorization_url }) => window.location.assign(authorization_url),
  });
  const fields = start.error instanceof ApiError ? start.error.fieldErrors : {};
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (password) start.mutate();
  };
  return (
    <form onSubmit={submit} className="grid max-w-sm gap-3 border-t pt-3">
      <p className="text-sm text-muted-foreground">
        Link your {provider} identity to sign in with it. Your password confirms it; you then sign
        in at {provider} and come back here. Whoever signs in there can then sign in to this
        account.
      </p>
      <div className="grid gap-1.5">
        <Label htmlFor="link-password">Password</Label>
        <Input
          id="link-password"
          type="password"
          autoComplete="current-password"
          value={password}
          aria-invalid={Boolean(fields.password)}
          onChange={(e) => setPassword(e.target.value)}
        />
        {fields.password ? (
          <p role="alert" className="text-xs text-destructive">
            {fields.password.join(' ')}
          </p>
        ) : start.error ? (
          <p role="alert" className="text-xs text-destructive">
            {errorMessage(start.error)}
          </p>
        ) : null}
      </div>
      <Button
        type="submit"
        variant="outline"
        size="sm"
        className="justify-self-start"
        disabled={!password || start.isPending}
      >
        <KeyRoundIcon aria-hidden />
        Link {provider}
      </Button>
    </form>
  );
}
