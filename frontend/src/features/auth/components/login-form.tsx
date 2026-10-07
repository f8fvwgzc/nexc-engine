import { zodResolver } from '@hookform/resolvers/zod';
import { useForm } from 'react-hook-form';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { loginInputSchema, type LoginInput } from '@/schemas/auth';

import { login } from '../api';
import { useAuthSubmit } from '../hooks/use-auth-submit';
import { FormAlert } from './form-alert';

export function LoginForm() {
  const form = useForm<LoginInput>({
    resolver: zodResolver(loginInputSchema),
    defaultValues: { email: '', password: '', code: '' },
    mode: 'onTouched',
  });
  const mutation = useAuthSubmit({
    submit: login,
    setError: form.setError,
    fields: ['email', 'password', 'code'],
    statusMessages: { 401: 'Incorrect email or password.' },
  });

  return (
    <form
      noValidate
      onSubmit={form.handleSubmit((values) =>
        mutation.mutate({ ...values, code: values.code?.trim() || undefined }),
      )}
    >
      <FieldGroup>
        <FormAlert message={form.formState.errors.root?.server?.message} />
        <FormField control={form.control} name="email" label="Email">
          {(field) => (
            <Input {...field} type="email" autoComplete="email" placeholder="you@example.com" />
          )}
        </FormField>
        <FormField control={form.control} name="password" label="Password">
          {(field) => <Input {...field} type="password" autoComplete="current-password" />}
        </FormField>
        {/* Shown once the server says this account signs in with a second factor. */}
        {(form.formState.errors.code ?? form.getValues('code')) ? (
          <FormField control={form.control} name="code" label="Authenticator code">
            {(field) => (
              <Input
                {...field}
                autoFocus
                inputMode="numeric"
                autoComplete="one-time-code"
                placeholder="6 digits, or a recovery code"
              />
            )}
          </FormField>
        ) : null}
        <AnimatedButton
          type="submit"
          size="lg"
          glow
          className="w-full"
          loading={mutation.isPending}
          loadingText="Signing in…"
        >
          Sign in
        </AnimatedButton>
      </FieldGroup>
    </form>
  );
}
