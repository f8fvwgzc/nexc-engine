import { ArrowUpRightIcon, ChevronLeftIcon, ShieldIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar';
import { CommandPalette } from '@/features/command/components/command-palette';
import { ShortcutsDialog } from '@/features/command/components/shortcuts-dialog';
import { useGlobalShortcuts } from '@/features/command/hooks/use-global-shortcuts';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { useAuthStore } from '@/stores/auth-store';

import { DemoBanner } from './demo-banner';
import { API_DOCS_ITEM } from './nav-items';
import { PageSkeleton } from './page-skeleton';
import { SETTINGS_GROUPS } from './settings-nav';

/**
 * The settings area: its own screen in place of the app's, with a sidebar of settings pages and
 * a way back. Account pages are the member's own; workspace pages are about the open workspace.
 */
export default function SettingsShell() {
  useGlobalShortcuts();
  const { pathname } = useLocation();
  const { current } = useCurrentWorkspace();
  const admin = current?.role === 'owner' || current?.role === 'admin';
  const platformAdmin = useAuthStore((s) => s.user?.role === 'admin');
  return (
    <SidebarProvider open>
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      <Sidebar>
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton asChild className="h-8 text-[13px] font-medium">
                <Link to="/app/issues">
                  <ChevronLeftIcon />
                  <span>Back to app</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>
        <SidebarContent>
          {SETTINGS_GROUPS.map((group) => (
            <SidebarGroup key={group.label} className="py-1">
              <SidebarGroupLabel className="h-7" title={group.hint}>
                {group.label === 'Workspace' && current ? current.name : group.label}
              </SidebarGroupLabel>
              <SidebarMenu className="gap-0.5">
                {group.items
                  .filter((item) => admin || !item.adminOnly)
                  .map((item) => (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton
                        asChild
                        isActive={pathname.startsWith(item.to)}
                        className="h-7 text-[13px]"
                      >
                        <NavLink to={item.to}>
                          <item.icon />
                          <span>{item.title}</span>
                        </NavLink>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
              </SidebarMenu>
            </SidebarGroup>
          ))}
        </SidebarContent>
        <SidebarFooter>
          <SidebarMenu>
            {platformAdmin && (
              <SidebarMenuItem>
                <SidebarMenuButton asChild className="h-7 text-[13px]">
                  <Link to="/app/platform">
                    <ShieldIcon />
                    <span>Platform console</span>
                  </Link>
                </SidebarMenuButton>
              </SidebarMenuItem>
            )}
            <SidebarMenuItem>
              <SidebarMenuButton asChild className="h-7 text-[13px]">
                <a href={API_DOCS_ITEM.to} target="_blank" rel="noopener noreferrer">
                  <API_DOCS_ITEM.icon />
                  <span>{API_DOCS_ITEM.title}</span>
                  <ArrowUpRightIcon className="ml-auto opacity-50" aria-hidden />
                  <span className="sr-only">(opens in a new tab)</span>
                </a>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset id="main" className="h-svh min-w-0 overflow-hidden">
        {/* Only a phone needs a bar: it holds the button that opens the settings sidebar. */}
        <header className="flex h-11 shrink-0 items-center gap-2 border-b px-3 md:hidden">
          <SidebarTrigger className="-ml-1" aria-label="Open settings navigation" />
          <span className="text-[13px] font-medium">Settings</span>
        </header>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <Suspense fallback={<PageSkeleton />}>
            <Outlet />
          </Suspense>
        </div>
      </SidebarInset>
      <DemoBanner />
      <CommandPalette />
      <ShortcutsDialog />
    </SidebarProvider>
  );
}
