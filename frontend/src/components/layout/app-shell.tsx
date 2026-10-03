import { Suspense } from 'react';
import { Outlet } from 'react-router-dom';

import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { CommandPalette } from '@/features/command/components/command-palette';
import { ShortcutsDialog } from '@/features/command/components/shortcuts-dialog';
import { useGlobalShortcuts } from '@/features/command/hooks/use-global-shortcuts';

import { AppHeader } from './app-header';
import { AppSidebar } from './app-sidebar';
import { DemoBanner } from './demo-banner';
import { PageSkeleton } from './page-skeleton';

function sidebarDefaultOpen(): boolean {
  return !document.cookie.split('; ').includes('sidebar_state=false');
}

/** Logged-in shell: sidebar + header + routed content, palette and global shortcuts. */
export default function AppShell() {
  useGlobalShortcuts();
  return (
    <SidebarProvider defaultOpen={sidebarDefaultOpen()}>
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      <AppSidebar />
      {/* SidebarInset renders the page's <main> landmark. */}
      <SidebarInset id="main" className="min-w-0">
        <AppHeader />
        <div className="flex min-h-0 flex-1 flex-col">
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
