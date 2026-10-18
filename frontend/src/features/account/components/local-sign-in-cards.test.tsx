import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { SignInMethods } from '@/schemas/auth';

import { LocalSignInCards } from './local-sign-in-cards';

const { state } = vi.hoisted(() => ({ state: { methods: null as unknown } }));

vi.mock('@/features/account/api', () => ({
  signInMethodsQuery: () => ({
    queryKey: ['account', 'sign-in'],
    queryFn: () => Promise.resolve(state.methods),
  }),
  twoFactorQuery: () => ({
    queryKey: ['account', 'two-factor'],
    queryFn: () => Promise.resolve({ enabled: true, recovery_codes_left: 8 }),
  }),
  changePassword: vi.fn(),
  setupTwoFactor: vi.fn(),
  enableTwoFactor: vi.fn(),
  disableTwoFactor: vi.fn(),
}));

function renderWith(methods: SignInMethods) {
  state.methods = methods;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <LocalSignInCards />
    </QueryClientProvider>,
  );
}

const identity = {
  provider: 'Acme SSO',
  issuer: 'https://idp.example.com/realms/acme',
  email: 'ada@example.com',
  linked_at: '2026-10-01T10:00:00Z',
  last_login_at: '2026-10-18T09:00:00Z',
};

describe('LocalSignInCards', () => {
  it('offers the password and the second factor to an account with a password', async () => {
    renderWith({ password: true, identities: [identity] });
    expect(await screen.findByLabelText('Current password')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Two-factor sign-in' })).toBeInTheDocument();
  });

  it('says the provider checks both for an account single sign-on created', async () => {
    renderWith({ password: false, identities: [identity] });
    expect(await screen.findByText(/you sign in with Acme SSO/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Current password')).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Two-factor sign-in' })).not.toBeInTheDocument();
  });
});
