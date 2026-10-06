import { Building2Icon, type LucideIcon, ServerIcon, UsersIcon } from 'lucide-react';
import { Suspense } from 'react';
import { Navigate, NavLink, Outlet, useLocation } from 'react-router-dom';

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
import { useAuthStore } from '@/stores/auth-store';

import { BrandMark } from './brand-mark';
import { NavUser } from './nav-user';
import { PageSkeleton } from './page-skeleton';
import { ThemeToggle } from './theme-toggle';

interface PlatformGroup {
  label: string;
  items: { title: string; to: string; icon: LucideIcon }[];
}

/** The console's own menu: nothing of a workspace is in it. */
const GROUPS: PlatformGroup[] = [
  {
    label: 'Tenants',
    items: [
      { title: 'Workspaces', to: '/app/platform/workspaces', icon: Building2Icon },
      { title: 'Accounts', to: '/app/platform/users', icon: UsersIcon },
    ],
  },
  {
    label: 'Server',
    items: [{ title: 'Infrastructure', to: '/app/platform/infrastructure', icon: ServerIcon }],
  },
];

/**
 * The platform console: where a platform administrator works. It has the app's layout (sidebar,
 * header, account menu) with its own menu, and is the only thing such an account sees: the
 * workspace app and its settings send a platform administrator here, and this sends everyone
 * else back to their workspace. It talks to `/admin/*` only.
 */
export default function PlatformShell() {
  const { pathname } = useLocation();
  const admin = useAuthStore((s) => s.user?.role === 'admin');
  if (!admin) return <Navigate to="/app/issues" replace />;
  const current = GROUPS.flatMap((g) => g.items).find((item) => pathname.startsWith(item.to));
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
          <div className="flex items-center gap-2 px-2 py-1.5">
            <BrandMark className="size-8 shrink-0" />
            <div className="grid min-w-0 flex-1 text-left text-sm leading-tight">
              <span className="truncate font-semibold">Nexc Platform</span>
              <span className="truncate text-xs text-muted-foreground">Administrator</span>
            </div>
          </div>
        </SidebarHeader>
        <SidebarContent>
          {GROUPS.map((group) => (
            <SidebarGroup key={group.label} className="py-1">
              <SidebarGroupLabel className="h-7">{group.label}</SidebarGroupLabel>
              <SidebarMenu className="gap-0.5">
                {group.items.map((item) => (
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
          <NavUser />
        </SidebarFooter>
      </Sidebar>
      <SidebarInset id="main" className="h-svh min-w-0 overflow-hidden">
        <header className="sticky top-0 z-20 flex h-11 shrink-0 items-center gap-2 border-b bg-background/80 px-3 backdrop-blur-lg sm:px-4">
          <SidebarTrigger className="-ml-1 md:hidden" aria-label="Open navigation" />
          <span className="text-[13px] text-muted-foreground">Platform</span>
          {current && <span className="text-[13px] font-medium">/ {current.title}</span>}
          <div className="ml-auto">
            <ThemeToggle />
          </div>
        </header>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <Suspense fallback={<PageSkeleton />}>
            <Outlet />
          </Suspense>
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
