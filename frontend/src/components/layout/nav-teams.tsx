import { useQuery } from '@tanstack/react-query';
import { NavLink, useLocation } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';
import { teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';

import { ROW } from './nav-section';

/** The teams the member belongs to, each a shortcut to that team's issues. */
export function NavTeams() {
  const { current } = useCurrentWorkspace();
  const { search, pathname } = useLocation();
  const { data: teams = [] } = useQuery({
    ...teamsQuery(current?.id ?? ''),
    enabled: Boolean(current),
  });
  const mine = teams.filter((team) => team.role !== null);
  if (mine.length === 0) return null;
  const openTeam = pathname === '/app/issues' ? new URLSearchParams(search).get('team') : null;
  return (
    <SidebarGroup className="py-1">
      <SidebarGroupLabel className="h-7">Your teams</SidebarGroupLabel>
      <SidebarMenu className="gap-0.5">
        {mine.map((team) => (
          <SidebarMenuItem key={team.id}>
            <SidebarMenuButton asChild isActive={openTeam === team.id} className={ROW}>
              <NavLink to={`/app/issues?team=${team.id}`}>
                <span className="flex size-4 shrink-0 items-center justify-center rounded-[4px] bg-foreground/10 text-[9px] font-semibold">
                  {team.key.slice(0, 1)}
                </span>
                <span>{team.name}</span>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        ))}
      </SidebarMenu>
    </SidebarGroup>
  );
}
