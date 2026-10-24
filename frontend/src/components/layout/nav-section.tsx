import { useQuery } from '@tanstack/react-query';
import { Link, useLocation } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';
import { teamsQuery } from '@/features/workspaces/api';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { itemMatches, type NavItem } from './nav-items';

/** Compact rows: 28px tall, 13px text. */
export const ROW = 'h-7 text-[13px]';

/** Lists that a `?team=` narrows to one team's (see nav-teams.tsx). */
const TEAM_LISTS = ['/app/issues', '/app/projects'];

/**
 * Whether the open page is the issues or projects of one of the member's own teams, whose row
 * under "Your teams" is highlighted instead. Another team's (one the member can see but has not
 * joined) has no row there, so everyone's list stays highlighted. It reads the teams nav-teams.tsx
 * already loaded and never asks for them itself: the platform console has this panel too.
 */
function useOwnTeamOpen(pathname: string, search: string): boolean {
  const workspaceId = useWorkspaceStore((s) => s.currentId);
  const teamId = TEAM_LISTS.includes(pathname) ? new URLSearchParams(search).get('team') : null;
  const { data: teams = [] } = useQuery({ ...teamsQuery(workspaceId ?? ''), enabled: false });
  return teamId !== null && teams.some((team) => team.id === teamId && team.role !== null);
}

interface NavSectionProps {
  label?: string;
  items: NavItem[];
  /** Counts shown at the end of a row and in the row's name, by the row's address. */
  badges?: Record<string, number>;
}

/** One group of the panel's rows. */
export function NavSection({ label, items, badges = {} }: NavSectionProps) {
  const { pathname, search } = useLocation();
  const teamOpen = useOwnTeamOpen(pathname, search);
  // A folder of artifacts is highlighted in the folder tree, not as all the files.
  const folderOpen = pathname === '/app/artifacts' && new URLSearchParams(search).has('folder');
  if (items.length === 0) return null;
  return (
    <SidebarGroup className="py-1">
      {label && <SidebarGroupLabel className="h-7">{label}</SidebarGroupLabel>}
      <SidebarMenu className="gap-0.5">
        {items.map((item) => {
          const count = badges[item.to] ?? 0;
          const active =
            itemMatches(item, pathname) &&
            !(teamOpen && item.to === pathname) &&
            !(folderOpen && item.to === '/app/artifacts');
          return (
            <SidebarMenuItem key={item.to}>
              {/* Marked current by the same rule as the highlight: a team's page marks its row
                  under "Your teams" instead. */}
              <SidebarMenuButton asChild isActive={active} className={ROW}>
                <Link to={item.to} aria-current={active ? 'page' : undefined}>
                  <item.icon />
                  <span>
                    {item.title}
                    {count > 0 && <span className="sr-only"> ({count} unread)</span>}
                  </span>
                </Link>
              </SidebarMenuButton>
              {count > 0 && <SidebarMenuBadge aria-hidden>{count}</SidebarMenuBadge>}
            </SidebarMenuItem>
          );
        })}
      </SidebarMenu>
    </SidebarGroup>
  );
}
