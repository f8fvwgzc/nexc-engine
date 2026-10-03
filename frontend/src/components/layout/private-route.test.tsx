import { render, screen } from '@testing-library/react';
import { createMemoryRouter, RouterProvider, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';

import { useAuthStore } from '@/stores/auth-store';
import * as f from '@/test/fixtures';

import { PrivateRoute } from './private-route';
import { PublicRoute } from './public-route';

function LocationProbe() {
  const location = useLocation();
  return <p data-testid="location">{`${location.pathname}${location.search}`}</p>;
}

function renderAt(path: string) {
  const router = createMemoryRouter(
    [
      {
        path: '/login',
        element: <PublicRoute />,
        children: [{ index: true, element: <LocationProbe /> }],
      },
      {
        path: '/app',
        element: <PrivateRoute />,
        children: [
          { path: '*', element: <p>secret area</p> },
          { index: true, element: <LocationProbe /> },
        ],
      },
    ],
    { initialEntries: [path] },
  );
  return render(<RouterProvider router={router} />);
}

beforeEach(() => {
  useAuthStore.setState({ status: 'anonymous', user: null, accessToken: null, expiresAt: null });
});

describe('PrivateRoute', () => {
  it('redirects anonymous users to /login with the original path in ?next=', async () => {
    renderAt('/app/agents?tab=org');
    expect(await screen.findByTestId('location')).toHaveTextContent(
      '/login?next=%2Fapp%2Fagents%3Ftab%3Dorg',
    );
    expect(screen.queryByText('secret area')).not.toBeInTheDocument();
  });

  it('shows the boot splash while the session is being restored', () => {
    useAuthStore.setState({ status: 'booting' });
    renderAt('/app/agents');
    expect(screen.getByRole('status', { name: /loading nexc-engine/i })).toBeInTheDocument();
  });

  it('renders the protected route when authenticated', async () => {
    useAuthStore.getState().setSession(f.authResponse);
    renderAt('/app/agents');
    expect(await screen.findByText('secret area')).toBeInTheDocument();
  });
});

describe('PublicRoute', () => {
  it('sends authenticated users to ?next= (same-origin paths only)', async () => {
    useAuthStore.getState().setSession(f.authResponse);
    renderAt('/login?next=//evil.example');
    expect(await screen.findByTestId('location')).toHaveTextContent(/^\/app$/);
  });
});
