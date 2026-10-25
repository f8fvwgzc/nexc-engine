import { Suspense } from 'react';
import { Navigate, Outlet, useLocation } from 'react-router-dom';

import { MainLandmarkProvider } from '@/components/custom-ui/main-landmark';
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar';
import { useAuthStore } from '@/stores/auth-store';

import { itemMatches, PLATFORM_MENUS } from './nav-items';
import { SettingsSkeleton } from './page-skeleton';
import { ShellSidebar } from './shell-sidebar';
import { ThemeToggle } from './theme-toggle';
import { SHELL_STYLE, usePanelOpen } from './panel';

/**
 * The platform console: where a platform administrator works. It has the app's layout (the
 * rail, the panel, the account menu) with its own menu, and is the only thing such an account
 * sees: the workspace app sends a platform administrator here, and this sends everyone else
 * back to their workspace. It talks to `/admin/*` only.
 */
export default function PlatformShell() {
  const { pathname } = useLocation();
  const [open, setOpen] = usePanelOpen();
  const admin = useAuthStore((s) => s.user?.role === 'admin');
  if (!admin) return <Navigate to="/app/issues" replace />;
  const current = PLATFORM_MENUS.flatMap((menu) => menu.groups)
    .flatMap((group) => group.items)
    .find((item) => itemMatches(item, pathname));
  return (
    <SidebarProvider open={open} onOpenChange={setOpen} style={SHELL_STYLE}>
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      {/* The panel's title row reads "Console"; the account menu says who administers it. */}
      <ShellSidebar menus={PLATFORM_MENUS} home="/app/platform/workspaces" />
      <SidebarInset id="main" className="h-svh min-w-0 overflow-hidden">
        <header className="sticky top-0 z-20 flex h-11 shrink-0 items-center gap-2 border-b bg-background/80 px-3 backdrop-blur-lg sm:px-4">
          <SidebarTrigger className="-ml-1" aria-label="Toggle navigation" />
          <span className="text-[13px] text-muted-foreground">Platform</span>
          {current && <span className="text-[13px] font-medium">/ {current.title}</span>}
          <div className="ml-auto">
            <ThemeToggle />
          </div>
        </header>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <MainLandmarkProvider>
            <Suspense fallback={<SettingsSkeleton />}>
              <Outlet />
            </Suspense>
          </MainLandmarkProvider>
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
