import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { Button } from '@/components/ui/button';
import { providersQuery } from '@/features/auth/api';
import { AuthCard } from '@/features/auth/components/auth-card';
import { RegisterForm } from '@/features/auth/components/register-form';

export default function RegisterPage() {
  const { data: providers } = useQuery(providersQuery());
  // Until the installation says otherwise, the form is what this page has always offered.
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
        {closed ? (
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
