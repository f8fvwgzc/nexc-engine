import { useQuery } from '@tanstack/react-query';

import { GlassCard } from '@/components/custom-ui/glass-card';

import { signInMethodsQuery } from '../api';
import { PasswordCard } from './password-card';
import { TwoFactorCard } from './two-factor-card';

/**
 * The password and the local second factor, for an account that has a password. One that single
 * sign-on created has neither here: its identity provider checks both, so a note says so instead
 * of forms it could never use.
 */
export function LocalSignInCards() {
  const { data } = useQuery(signInMethodsQuery());
  if (data?.password === false) {
    const provider = data.identities[0]?.provider ?? 'your organisation’s identity provider';
    return (
      <GlassCard id="password" className="scroll-mt-20 space-y-1 p-5 sm:p-6">
        <h2 className="font-medium">Password and two-factor sign-in</h2>
        <p className="text-sm text-muted-foreground">
          This account has no password here: you sign in with {provider}, which checks your password
          and asks for any second factor. Change them there.
        </p>
      </GlassCard>
    );
  }
  return (
    <>
      <PasswordCard />
      <TwoFactorCard />
    </>
  );
}
