import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import type { LoginProviders, SignInMethods } from '@/schemas/auth';

import { SignInCard } from './sign-in-card';

const { state, linkIdentity } = vi.hoisted(() => {
  const state: { methods: unknown; providers: unknown } = { methods: null, providers: null };
  // Never answers: the browser would leave for the provider.
  return { state, linkIdentity: vi.fn(() => new Promise(() => {})) };
});

vi.mock('@/features/account/api', () => ({
  signInMethodsQuery: () => ({
    queryKey: ['account', 'sign-in'],
    queryFn: () => Promise.resolve(state.methods),
  }),
  linkIdentity,
}));

vi.mock('@/features/auth/api', () => ({
  providersQuery: () => ({
    queryKey: ['auth', 'providers'],
    queryFn: () => Promise.resolve(state.providers),
  }),
}));

const acme = { id: 'oidc', display_name: 'Acme SSO', login_url: '/api/v1/auth/oidc/login' };
const ssoOn: LoginProviders = { password: true, signup: true, sso: [acme] };
const identity = {
  provider: 'Acme SSO',
  issuer: 'https://idp.example.com/realms/acme',
  current: true,
  email: 'ada@example.com',
  linked_at: '2026-10-01T10:00:00Z',
  last_login_at: null,
};

function renderAt(path: string, methods: SignInMethods, providers: LoginProviders = ssoOn) {
  state.methods = methods;
  state.providers = providers;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <SignInCard />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  const loaded = () =>
    waitFor(() => {
      expect(client.getQueryState(['account', 'sign-in'])?.status).toBe('success');
      expect(client.getQueryState(['auth', 'providers'])?.status).toBe('success');
    });
  return { ...view, loaded };
}

describe('SignInCard', () => {
  it('links the provider to an account with a password, which confirms it', async () => {
    renderAt('/app/settings/profile', { password: true, identities: [] });
    const button = await screen.findByRole('button', { name: 'Link Acme SSO' });
    expect(button).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'my password' } });
    fireEvent.click(button);
    await waitFor(() => expect(linkIdentity).toHaveBeenCalledWith('my password'));
  });

  it('says why the provider sent the browser back without linking', async () => {
    renderAt('/app/settings/profile?sso_error=identity_taken', { password: true, identities: [] });
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'That Acme SSO identity is linked to another account here',
    );
  });

  it('lists a linked identity and offers no second one', async () => {
    renderAt('/app/settings/profile', { password: true, identities: [identity] });
    expect(await screen.findByText('ada@example.com')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Link/ })).not.toBeInTheDocument();
  });

  it('shows nothing while single sign-on is off and nothing is linked', async () => {
    const { container, loaded } = renderAt(
      '/app/settings/profile',
      { password: true, identities: [] },
      { password: true, signup: true, sso: [] },
    );
    await loaded();
    expect(container).toBeEmptyDOMElement();
  });

  it('offers no link to an account without a password', async () => {
    const { loaded } = renderAt('/app/settings/profile', {
      password: false,
      identities: [{ ...identity, current: false, provider: 'https://old.example.com' }],
    });
    await loaded();
    expect(await screen.findByText('ada@example.com')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Link/ })).not.toBeInTheDocument();
  });
});
