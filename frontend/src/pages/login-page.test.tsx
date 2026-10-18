import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import type { LoginProviders } from '@/schemas/auth';

import LoginPage from './login-page';

const { api, state } = vi.hoisted(() => {
  const state = { providers: null as unknown };
  return {
    state,
    api: {
      providersQuery: vi.fn(() => ({
        queryKey: ['auth', 'providers'],
        queryFn: () => Promise.resolve(state.providers),
      })),
      ssoLoginUrl: (next: string) =>
        `/api/v1/auth/oidc/login?return_to=${encodeURIComponent(next)}`,
      login: vi.fn(),
      register: vi.fn(),
    },
  };
});

vi.mock('@/features/auth/api', () => api);

const acme = { id: 'oidc', display_name: 'Acme SSO', login_url: '/api/v1/auth/oidc/login' };

function renderAt(path: string, providers: LoginProviders) {
  state.providers = providers;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <LoginPage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('LoginPage', () => {
  it('offers only the password form while single sign-on is off', async () => {
    renderAt('/login', { password: true, signup: true, sso: [] });
    expect(await screen.findByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /continue with/i })).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Create an account' })).toBeInTheDocument();
  });

  it('offers single sign-on above the password form when both are on', async () => {
    renderAt('/login?next=/app/runs', { password: true, signup: false, sso: [acme] });
    const button = await screen.findByRole('link', { name: 'Continue with Acme SSO' });
    expect(button).toHaveAttribute('href', '/api/v1/auth/oidc/login?return_to=%2Fapp%2Fruns');
    expect(button.querySelector('svg')).not.toBeNull();
    expect(screen.getByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Create an account' })).not.toBeInTheDocument();
  });

  it('hides the password form when only single sign-on is allowed', async () => {
    renderAt('/login?next=//evil.example', { password: false, signup: false, sso: [acme] });
    const button = await screen.findByRole('link', { name: 'Continue with Acme SSO' });
    expect(button).toHaveAttribute('href', '/api/v1/auth/oidc/login?return_to=%2Fapp');
    expect(screen.queryByLabelText('Email')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Password')).not.toBeInTheDocument();
    expect(screen.queryByText(/forgot your password/i)).not.toBeInTheDocument();
  });

  it('still lets the break-glass administrator in with a password', async () => {
    renderAt('/login?break_glass=1', { password: false, signup: false, sso: [acme] });
    expect(await screen.findByLabelText('Email')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Continue with Acme SSO' })).toBeInTheDocument();
  });

  it('says why single sign-on sent the browser back', async () => {
    renderAt('/login?sso_error=unverified_email', { password: true, signup: true, sso: [acme] });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Acme SSO has not verified your e-mail address',
    );
  });
});
