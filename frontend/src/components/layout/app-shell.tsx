import { Suspense } from 'react';
import { Outlet } from 'react-router-dom';

import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar';
import { CommandPalette } from '@/features/command/components/command-palette';
import { ShortcutsDialog } from '@/features/command/components/shortcuts-dialog';
import { useGlobalShortcuts } from '@/features/command/hooks/use-global-shortcuts';

import { AppHeader } from './app-header';
import { AppSidebar } from './app-sidebar';
import { AssistantDot } from './assistant-dot';
import { DemoBanner } from './demo-banner';
import { PageSkeleton } from './page-skeleton';

/** Logged-in shell: sidebar + header + routed content, palette and global shortcuts. */
export default function AppShell() {
  useGlobalShortcuts();
  return (
    // The sidebar stays open on desktop: there is nothing to collapse it. On a phone it is a drawer.
    <SidebarProvider open>
      <a
        href="#main"
        className="sr-only z-50 rounded-md bg-background px-3 py-2 focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      <AppSidebar />
      {/* SidebarInset renders the page's <main> landmark. It is exactly one viewport tall:
          pages scroll inside it, so a full-height page (the canvas) never scrolls the window. */}
      <SidebarInset
        id="main"
        className="h-svh min-w-0 overflow-hidden md:peer-data-[variant=inset]:h-[calc(100svh-1rem)]"
      >
        <AppHeader />
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <Suspense fallback={<PageSkeleton />}>
            <Outlet />
          </Suspense>
        </div>
      </SidebarInset>
      <AssistantDot />
      <DemoBanner />
      <CommandPalette />
      <ShortcutsDialog />
    </SidebarProvider>
  );
}
