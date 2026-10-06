import { Building2Icon, ChevronLeftIcon, ServerIcon, ShieldIcon, UsersIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Link, NavLink, Outlet, useLocation } from 'react-router-dom';

import { EmptyState } from '@/components/custom-ui/empty-state';
import {
  Sidebar,
  SidebarContent,
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
import { useAuthStore } from '@/stores/auth-store';

import { PageSkeleton } from './page-skeleton';

const ITEMS = [
  { title: 'Workspaces', to: '/app/platform/workspaces', icon: Building2Icon },
  { title: 'Accounts', to: '/app/platform/users', icon: UsersIcon },
  { title: 'Infrastructure', to: '/app/platform/infrastructure', icon: ServerIcon },
];

/**
 * The platform console: its own screen for whoever runs this installation, apart from any one
 * workspace. Accounts without the platform administrator role see why they cannot enter.
 */
export default function PlatformShell() {
  const { pathname } = useLocation();
  const admin = useAuthStore((s) => s.user?.role === 'admin');
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
          <SidebarGroup className="py-1">
            <SidebarGroupLabel className="h-7">Nexc platform</SidebarGroupLabel>
            <SidebarMenu className="gap-0.5">
              {ITEMS.map((item) => (
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
        </SidebarContent>
      </Sidebar>
      <SidebarInset id="main" className="h-svh min-w-0 overflow-hidden">
        <header className="flex h-11 shrink-0 items-center gap-2 border-b px-3 md:hidden">
          <SidebarTrigger className="-ml-1" aria-label="Open platform navigation" />
          <span className="text-[13px] font-medium">Platform</span>
        </header>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {admin ? (
            <Suspense fallback={<PageSkeleton />}>
              <Outlet />
            </Suspense>
          ) : (
            <div className="p-6">
              <EmptyState
                icon={ShieldIcon}
                title="For platform administrators"
                description="This console is about the whole installation: every workspace and account. Your account administers its own workspaces, not the platform."
              />
            </div>
          )}
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
