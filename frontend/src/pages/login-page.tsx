import { useQuery } from '@tanstack/react-query';
import { KeyRoundIcon } from 'lucide-react';
import { Link, useSearchParams } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { providersQuery, ssoLoginUrl } from '@/features/auth/api';
import { AuthCard } from '@/features/auth/components/auth-card';
import { FormAlert } from '@/features/auth/components/form-alert';
import { LoginForm } from '@/features/auth/components/login-form';
import { ssoErrorMessage } from '@/features/auth/sso-errors';
import { safeNextPath } from '@/lib/utils';

/**
 * Sign-in: "Continue with <provider>" when the installation has single sign-on, and the password
 * form unless it turned passwords off (the break-glass administrator opens `/login?break_glass=1`).
 */
export default function LoginPage() {
  const [params] = useSearchParams();
  const next = params.get('next');
  const ssoError = params.get('sso_error');
  const { data: providers, isPending } = useQuery(providersQuery());
  const sso = providers?.sso[0];
  // Should the question fail, the password form is what this page has always offered.
  const password = !providers || providers.password || params.has('break_glass');
  const signup = providers?.signup ?? true;
  return (
    <>
      <Seo title="Sign in" description="Sign in to your nexc-engine workspace." />
      <AuthCard
        title="Welcome back"
        description="Sign in to your graph workspace."
        footer={
          <>
            {signup ? (
              <>
                New to nexc?{' '}
                <Link
                  to={next ? `/register?next=${encodeURIComponent(next)}` : '/register'}
                  className="font-medium text-foreground underline-offset-4 hover:underline"
                >
                  Create an account
                </Link>
              </>
            ) : null}
            {password ? (
              <span className={signup ? 'mt-2 block text-xs' : 'block text-xs'}>
                Forgot your password? Ask whoever administers this installation for a reset link.
              </span>
            ) : null}
          </>
        }
      >
        {isPending ? (
          <Skeleton className="h-48 w-full rounded-lg" />
        ) : (
          <div className="space-y-4">
            {ssoError ? (
              <FormAlert
                message={ssoErrorMessage(ssoError, sso?.display_name ?? 'single sign-on')}
              />
            ) : null}
            {sso ? (
              // The one way in is the primary action; next to the form it is the outlined one.
              <Button
                asChild
                variant={password ? 'outline' : 'default'}
                size="lg"
                className="w-full"
              >
                <a href={ssoLoginUrl(safeNextPath(next))}>
                  <KeyRoundIcon aria-hidden />
                  Continue with {sso.display_name}
                </a>
              </Button>
            ) : null}
            {sso && password ? (
              <div className="flex items-center gap-3 text-xs text-muted-foreground" aria-hidden>
                <span className="h-px flex-1 bg-border" />
                or
                <span className="h-px flex-1 bg-border" />
              </div>
            ) : null}
            {password ? <LoginForm /> : null}
          </div>
        )}
      </AuthCard>
    </>
  );
}
