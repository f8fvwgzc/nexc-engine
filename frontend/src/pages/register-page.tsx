import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { providersQuery } from '@/features/auth/api';
import { AuthCard } from '@/features/auth/components/auth-card';
import { RegisterForm } from '@/features/auth/components/register-form';

export default function RegisterPage() {
  const { data: providers, isPending } = useQuery(providersQuery());
  // Unless the installation says otherwise (or cannot say), the form is what this page offers.
  const closed = providers?.signup === false;
  const sso = providers?.sso[0];
  return (
    <>
      <Seo title="Create account" description="Create a free nexc-engine account." />
      <AuthCard
        title="Create your account"
        description="Plan and run graphs of LLM tasks in minutes."
        footer={
          <>
            Already have an account?{' '}
            <Link
              to="/login"
              className="font-medium text-foreground underline-offset-4 hover:underline"
            >
              Sign in
            </Link>
          </>
        }
      >
        {isPending ? (
          // The form's place while the installation says whether it takes sign-ups.
          <div role="status" aria-busy aria-label="Loading" className="space-y-4">
            {Array.from({ length: 3 }, (_, i) => (
              <div key={i} className="space-y-2">
                <Skeleton className="h-4 w-24" />
                <Skeleton className="h-9 w-full" />
              </div>
            ))}
            <Skeleton className="h-10 w-full" />
          </div>
        ) : closed ? (
          <div className="space-y-4 text-center">
            <p className="text-sm text-muted-foreground">
              {sso
                ? `Accounts here are created by signing in with ${sso.display_name}.`
                : 'This installation does not take sign-ups. Ask whoever administers it for an account.'}
            </p>
            <Button asChild size="lg" className="w-full">
              <Link to="/login">Go to sign in</Link>
            </Button>
          </div>
        ) : (
          <RegisterForm />
        )}
      </AuthCard>
    </>
  );
}
