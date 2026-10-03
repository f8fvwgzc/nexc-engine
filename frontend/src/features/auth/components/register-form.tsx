import { zodResolver } from '@hookform/resolvers/zod';
import { useForm } from 'react-hook-form';

import { AnimatedButton } from '@/components/custom-ui/animated-button';
import { FormField } from '@/components/custom-ui/form-field';
import { FieldGroup } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { PASSWORD_MIN_LENGTH, registerInputSchema, type RegisterInput } from '@/schemas/auth';

import { register } from '../api';
import { useAuthSubmit } from '../hooks/use-auth-submit';
import { FormAlert } from './form-alert';

export function RegisterForm() {
  const form = useForm<RegisterInput>({
    resolver: zodResolver(registerInputSchema),
    defaultValues: { name: '', email: '', password: '' },
    mode: 'onTouched',
  });
  const mutation = useAuthSubmit({
    submit: register,
    setError: form.setError,
    fields: ['name', 'email', 'password'],
    statusMessages: {
      403: 'Sign-ups are disabled on this server. Ask an admin for an account.',
      409: 'An account with this email already exists.',
    },
  });

  return (
    <form noValidate onSubmit={form.handleSubmit((values) => mutation.mutate(values))}>
      <FieldGroup>
        <FormAlert message={form.formState.errors.root?.server?.message} />
        <FormField control={form.control} name="name" label="Name">
          {(field) => <Input {...field} autoComplete="name" placeholder="Ada Lovelace" />}
        </FormField>
        <FormField control={form.control} name="email" label="Email">
          {(field) => (
            <Input {...field} type="email" autoComplete="email" placeholder="you@example.com" />
          )}
        </FormField>
        <FormField
          control={form.control}
          name="password"
          label="Password"
          description={`At least ${PASSWORD_MIN_LENGTH} characters. A passphrase works great.`}
        >
          {(field) => <Input {...field} type="password" autoComplete="new-password" />}
        </FormField>
        <AnimatedButton
          type="submit"
          size="lg"
          glow
          className="w-full"
          loading={mutation.isPending}
          loadingText="Creating account…"
        >
          Create account
        </AnimatedButton>
      </FieldGroup>
    </form>
  );
}
