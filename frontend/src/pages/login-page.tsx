import { Link, useSearchParams } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { AuthCard } from '@/features/auth/components/auth-card';
import { LoginForm } from '@/features/auth/components/login-form';

export default function LoginPage() {
  const [params] = useSearchParams();
  const next = params.get('next');
  return (
    <>
      <Seo title="Sign in" description="Sign in to your nexc-engine workspace." />
      <AuthCard
        title="Welcome back"
        description="Sign in to your graph workspace."
        footer={
          <>
            New to nexc?{' '}
            <Link
              to={next ? `/register?next=${encodeURIComponent(next)}` : '/register'}
              className="font-medium text-foreground underline-offset-4 hover:underline"
            >
              Create an account
            </Link>
            <span className="mt-2 block text-xs">
              Forgot your password? Ask whoever administers this installation for a reset link.
            </span>
          </>
        }
      >
        <LoginForm />
      </AuthCard>
    </>
  );
}
