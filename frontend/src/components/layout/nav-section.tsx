import { NavLink, useLocation } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';

import { itemMatches, type NavItem } from './nav-items';

/** Compact rows: 28px tall, 13px text. */
export const ROW = 'h-7 text-[13px]';

/** Lists that a `?team=` narrows to one team's (see nav-teams.tsx). */
const TEAM_LISTS = ['/app/issues', '/app/projects'];

interface NavSectionProps {
  label?: string;
  items: NavItem[];
  /** Counts shown at the end of a row and in the row's name, by the row's address. */
  badges?: Record<string, number>;
}

/** One group of the panel's rows. */
export function NavSection({ label, items, badges = {} }: NavSectionProps) {
  const { pathname, search } = useLocation();
  // A team's issues or projects are highlighted under "Your teams", not as everyone's.
  const teamOpen = TEAM_LISTS.includes(pathname) && new URLSearchParams(search).has('team');
  if (items.length === 0) return null;
  return (
    <SidebarGroup className="py-1">
      {label && <SidebarGroupLabel className="h-7">{label}</SidebarGroupLabel>}
      <SidebarMenu className="gap-0.5">
        {items.map((item) => {
          const count = badges[item.to] ?? 0;
          const active = itemMatches(item, pathname) && !(teamOpen && item.to === pathname);
          return (
            <SidebarMenuItem key={item.to}>
              <SidebarMenuButton asChild isActive={active} className={ROW}>
                <NavLink to={item.to} end={item.end}>
                  <item.icon />
                  <span>
                    {item.title}
                    {count > 0 && <span className="sr-only"> ({count} unread)</span>}
                  </span>
                </NavLink>
              </SidebarMenuButton>
              {count > 0 && <SidebarMenuBadge aria-hidden>{count}</SidebarMenuBadge>}
            </SidebarMenuItem>
          );
        })}
      </SidebarMenu>
    </SidebarGroup>
  );
}
