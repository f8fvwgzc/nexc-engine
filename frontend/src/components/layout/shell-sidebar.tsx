import type { ReactNode } from 'react';
import { useLocation } from 'react-router-dom';

import { Sidebar, SidebarContent, SidebarHeader } from '@/components/ui/sidebar';

import { ActivityRail } from './activity-rail';
import { activeMenu, type CoreMenu } from './nav-items';
import { NavSection } from './nav-section';

interface ShellSidebarProps {
  menus: CoreMenu[];
  /** Where the rail's brand mark goes. */
  home: string;
  /** The top of the panel: the workspace switcher, or the console's name. */
  header: ReactNode;
  /** Rows after a menu's own, by the menu's id: the member's teams, the recent graphs. */
  extras?: Record<string, ReactNode>;
  /** Counts by a row's address: at the end of the row, and a dot on its menu's icon. */
  badges?: Record<string, number>;
  /** Whether rows marked for admins are shown. */
  admin?: boolean;
}

/**
 * The sidebar of every signed-in screen: the rail of core menus, and beside it the panel with
 * the rows of the menu the open page belongs to. The panel is what the provider opens and
 * closes; the rail stays.
 */
export function ShellSidebar({
  menus,
  home,
  header,
  extras = {},
  badges = {},
  admin = true,
}: ShellSidebarProps) {
  const { pathname } = useLocation();
  const menu = activeMenu(menus, pathname);
  const shown = menu ?? menus[0];
  if (!shown) return null;
  const railBadges = Object.fromEntries(
    menus.map((m) => [
      m.id,
      m.groups.flatMap((g) => g.items).reduce((sum, item) => sum + (badges[item.to] ?? 0), 0),
    ]),
  );
  return (
    <Sidebar collapsible="icon">
      <div className="flex h-full min-h-0">
        <ActivityRail menus={menus} active={menu} home={home} badges={railBadges} />
        <nav
          aria-label={shown.title}
          className="flex min-w-0 flex-1 flex-col group-data-[collapsible=icon]:hidden"
        >
          <SidebarHeader>{header}</SidebarHeader>
          <SidebarContent className="gap-0">
            <div
              aria-hidden
              className="flex h-8 shrink-0 items-center px-4 text-[13px] font-semibold"
            >
              {shown.title}
            </div>
            {shown.groups.map((group, i) => (
              <NavSection
                key={group.label ?? i}
                label={group.label}
                items={group.items.filter((item) => admin || !item.adminOnly)}
                badges={badges}
              />
            ))}
            {extras[shown.id]}
          </SidebarContent>
        </nav>
      </div>
    </Sidebar>
  );
}
