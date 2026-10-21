import { Suspense } from 'react';
import { Navigate, Outlet, useLocation } from 'react-router-dom';

import { MainLandmarkProvider } from '@/components/custom-ui/main-landmark';
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { CommandPalette } from '@/features/command/components/command-palette';
import { ShortcutsDialog } from '@/features/command/components/shortcuts-dialog';
import { useGlobalShortcuts } from '@/features/command/hooks/use-global-shortcuts';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { isWorkspaceAdmin } from '@/schemas/workspace';
import { useAuthStore } from '@/stores/auth-store';

import { AppHeader } from './app-header';
import { AssistantDot } from './assistant-dot';
import { DemoBanner } from './demo-banner';
import { NavGraphs } from './nav-graphs';
import { WORKSPACE_MENUS } from './nav-items';
import { NavTeams } from './nav-teams';
import { PageSkeleton } from './page-skeleton';
import { ShellSidebar } from './shell-sidebar';
import { SHELL_STYLE, usePanelOpen } from './panel';
import { useUnread } from './use-unread';
import { WorkspaceSwitcher } from './workspace-switcher';

/**
 * The signed-in shell: the rail of core menus, the panel of the open menu's pages, the header
 * and the routed page, with the palette and the global shortcuts. The settings pages are in it
 * like every other page.
 */
export default function AppShell() {
  useGlobalShortcuts();
  const [open, setOpen] = usePanelOpen();
  const { pathname } = useLocation();
  const { current } = useCurrentWorkspace();
  const unread = useUnread();
  // A platform administrator works in the platform console, not in a workspace.
  const platformAdmin = useAuthStore((s) => s.user?.role === 'admin');
  if (platformAdmin) return <Navigate to="/app/platform" replace />;
  return (
    <SidebarProvider open={open} onOpenChange={setOpen} style={SHELL_STYLE}>
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      <ShellSidebar
        menus={WORKSPACE_MENUS}
        home="/app/inbox"
        header={<WorkspaceSwitcher />}
        admin={current ? isWorkspaceAdmin(current.role) : false}
        badges={{ '/app/inbox': unread }}
        extras={{ issues: <NavTeams />, graphs: <NavGraphs /> }}
      />
      {/* SidebarInset renders the page's <main> landmark. It is exactly one viewport tall:
          pages scroll inside it, so a full-height page (the canvas) never scrolls the window. */}
      <SidebarInset id="main" className="h-svh min-w-0 overflow-hidden">
        <AppHeader />
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <MainLandmarkProvider>
            {/* A settings page is narrow; so is its placeholder, so nothing moves when it arrives. */}
            <Suspense
              fallback={
                <PageSkeleton width={pathname.startsWith('/app/settings') ? 'narrow' : 'default'} />
              }
            >
              <Outlet />
            </Suspense>
          </MainLandmarkProvider>
        </div>
      </SidebarInset>
      <AssistantDot />
      <DemoBanner />
      <CommandPalette />
      <ShortcutsDialog />
    </SidebarProvider>
  );
}
