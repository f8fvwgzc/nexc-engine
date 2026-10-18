import { useQuery } from '@tanstack/react-query';
import { KeyRoundIcon } from 'lucide-react';

import { GlassCard } from '@/components/custom-ui/glass-card';
import { formatDateTime, formatRelative } from '@/lib/format';

import { signInMethodsQuery } from '../api';

/**
 * The single sign-on identities the account is linked to, and whether it also has a password.
 * Nothing shows for an account that only ever signed in with a password.
 */
export function SignInCard() {
  const { data } = useQuery(signInMethodsQuery());
  if (!data || data.identities.length === 0) return null;
  return (
    <GlassCard id="single-sign-on" className="scroll-mt-20 space-y-3 p-5 sm:p-6">
      <div>
        <h2 className="font-medium">Single sign-on</h2>
        <p className="text-sm text-muted-foreground">
          {data.password
            ? 'You sign in through your organisation’s identity provider, or with your password.'
            : 'You sign in through your organisation’s identity provider; this account has no password here.'}
        </p>
      </div>
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
    </GlassCard>
  );
}
