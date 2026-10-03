import { Link } from 'react-router-dom';

import { Seo } from '@/components/seo/seo';
import { AuthCard } from '@/features/auth/components/auth-card';
import { RegisterForm } from '@/features/auth/components/register-form';

export default function RegisterPage() {
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
        <RegisterForm />
      </AuthCard>
    </>
  );
}
