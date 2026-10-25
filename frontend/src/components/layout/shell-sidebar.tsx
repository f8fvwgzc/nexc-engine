import type { MouseEvent, ReactNode } from 'react';
import { useLocation } from 'react-router-dom';

import { Sidebar, SidebarContent, useSidebar } from '@/components/ui/sidebar';

import { ActivityRail } from './activity-rail';
import { activeMenu, type CoreMenu } from './nav-items';
import { NavSection } from './nav-section';

interface ShellSidebarProps {
  menus: CoreMenu[];
  /** Where the rail's brand mark goes. */
  home: string;
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
 * closes; the rail stays. The panel's top row is the menu's title, as tall as the page's header
 * beside it, and stays put while the rows under it scroll.
 */
export function ShellSidebar({
  menus,
  home,
  extras = {},
  badges = {},
  admin = true,
}: ShellSidebarProps) {
  const { pathname } = useLocation();
  const { isMobile, setOpenMobile } = useSidebar();
  const menu = activeMenu(menus, pathname);
  const shown = menu ?? menus[0];
  if (!shown) return null;
  const railBadges = Object.fromEntries(
    menus.map((m) => [
      m.id,
      m.groups.flatMap((g) => g.items).reduce((sum, item) => sum + (badges[item.to] ?? 0), 0),
    ]),
  );
  // On a phone the sidebar is a drawer over the page: following one of its links (a row, the
  // brand mark, an entry of the account menu) closes it, so the page shows. A menu of the rail
  // only swaps the rows beside it, so the drawer stays open for the reader to pick one.
  const closeOnNavigate = (event: MouseEvent) => {
    if (!isMobile || !(event.target instanceof Element)) return;
    const link = event.target.closest('a[href]');
    if (link && !link.closest('[data-rail-menu]') && link.getAttribute('target') !== '_blank') {
      setOpenMobile(false);
    }
  };
  return (
    <Sidebar collapsible="icon">
      {/* Clicks only bubble here from the links inside; the links stay the controls. */}
      <div className="flex h-full min-h-0" onClick={closeOnNavigate}>
        <ActivityRail menus={menus} active={menu} home={home} badges={railBadges} />
        <nav
          aria-label={shown.title}
          className="flex min-w-0 flex-1 flex-col group-data-[collapsible=icon]:hidden"
        >
          <div
            aria-hidden
            className="flex h-11 shrink-0 items-center border-b border-sidebar-border px-4 text-[13px] font-semibold"
          >
            {shown.title}
          </div>
          <SidebarContent className="gap-0 pt-1">
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
