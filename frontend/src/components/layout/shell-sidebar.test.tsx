import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ThemeContext } from '@/app/providers/theme-context';
import { SidebarProvider, useSidebar } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { qk } from '@/lib/query-keys';
import type { Workspace } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';
import { useWorkspaceStore } from '@/stores/workspace-store';
import * as f from '@/test/fixtures';

import { WORKSPACE_MENUS } from './nav-items';
import { ShellSidebar } from './shell-sidebar';

const phone = vi.hoisted(() => ({ is: false }));
vi.mock('@/hooks/use-mobile', () => ({ useIsMobile: () => phone.is }));

const workspace = (id: string, name: string, member_count: number): Workspace => ({
  id,
  name,
  slug: name.toLowerCase(),
  role: 'admin',
  member_count,
  created_at: '2026-10-01T09:00:00Z',
});
const ACME = workspace('0199eeee-0000-7000-8000-000000000001', 'Acme', 5);
const LABS = workspace('0199eeee-0000-7000-8000-000000000002', 'Labs', 1);

/** Where the router is, and whether the phone's drawer is open (with a way to open it). */
function Probe() {
  const { pathname } = useLocation();
  const { openMobile, setOpenMobile } = useSidebar();
  return (
    <>
      <output aria-label="Address">{pathname}</output>
      <output aria-label="Drawer">{openMobile ? 'open' : 'closed'}</output>
      <button type="button" onClick={() => setOpenMobile(true)}>
        Open the drawer
      </button>
    </>
  );
}

function renderShell() {
  const client = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity } } });
  client.setQueryData(qk.workspaces.all, [ACME, LABS]);
  client.setQueryData(qk.workspaces.teams(ACME.id), []);
  return render(
    <QueryClientProvider client={client}>
      <ThemeContext value={{ theme: 'system', resolvedTheme: 'light', setTheme: vi.fn() }}>
        <MemoryRouter initialEntries={['/app/issues']}>
          <TooltipProvider>
            <SidebarProvider>
              <Probe />
              <ShellSidebar menus={WORKSPACE_MENUS} home="/app/inbox" />
            </SidebarProvider>
          </TooltipProvider>
        </MemoryRouter>
      </ThemeContext>
    </QueryClientProvider>,
  );
}

/** Opens the account menu from the keyboard, as Radix menus open in jsdom. */
function openAccountMenu() {
  const trigger = screen.getByRole('button', { name: 'Account menu' });
  fireEvent.keyDown(trigger, { key: 'Enter' });
  return screen.getByRole('menu');
}

beforeEach(() => {
  phone.is = false;
  useAuthStore.getState().setSession(f.authResponse);
  useWorkspaceStore.getState().setCurrent(ACME.id);
});
afterEach(() => {
  useAuthStore.getState().clearSession();
  localStorage.clear();
});

describe('the account menu', () => {
  it('switches to another workspace from its workspaces submenu', async () => {
    renderShell();
    const menu = openAccountMenu();
    const submenu = within(menu).getByRole('menuitem', { name: 'Switch workspace (Acme, admin)' });
    fireEvent.keyDown(submenu, { key: 'ArrowRight' });
    const labs = await screen.findByRole('menuitemradio', { name: /Labs/ });
    expect(screen.getByRole('menuitemradio', { name: /Acme/ })).toHaveAttribute(
      'aria-checked',
      'true',
    );
    expect(labs).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByRole('menuitem', { name: 'New workspace…' })).toBeInTheDocument();

    fireEvent.click(labs);
    expect(useWorkspaceStore.getState().currentId).toBe(LABS.id);
    expect(screen.getByLabelText('Address')).toHaveTextContent(/^\/app$/);
  });

  it('has no workspaces for a platform administrator', () => {
    useAuthStore.getState().setSession({ ...f.authResponse, user: { ...f.user, role: 'admin' } });
    renderShell();
    const menu = openAccountMenu();
    expect(within(menu).getByText('Platform administrator')).toBeInTheDocument();
    expect(within(menu).queryByRole('menuitem', { name: /Switch workspace/ })).toBeNull();
  });
});

describe('the drawer on a phone', () => {
  it('closes when a row of the panel is followed, and stays for a menu of the rail', () => {
    phone.is = true;
    renderShell();
    const drawer = () => screen.getByLabelText('Drawer');
    act(() => screen.getByRole('button', { name: 'Open the drawer' }).click());
    expect(drawer()).toHaveTextContent('open');

    // A menu of the rail swaps the rows beside it: the reader still has to pick one.
    fireEvent.click(screen.getByRole('link', { name: 'Agents' }));
    expect(drawer()).toHaveTextContent('open');
    expect(screen.getByLabelText('Address')).toHaveTextContent('/app/agents');

    fireEvent.click(screen.getByRole('link', { name: 'Conversations' }));
    expect(drawer()).toHaveTextContent('closed');
    expect(screen.getByLabelText('Address')).toHaveTextContent('/app/conversations');
  });
});
