import { useQuery } from '@tanstack/react-query';
import { ArrowUpRightIcon } from 'lucide-react';
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

import { ADMIN_ITEMS, API_DOCS_ITEM, BUILD_ITEMS, WORK_ITEMS, type NavItem } from './nav-items';

/** Compact rows: 28px tall, 13px text. */
const ROW = 'h-7 text-[13px]';

function NavSection({ label, items }: { label?: string; items: NavItem[] }) {
  const { pathname, search } = useLocation();
  // A team's issue list is highlighted under "Your teams", not as "Issues".
  const teamOpen = pathname === '/app/issues' && new URLSearchParams(search).has('team');
  const isActive = (to: string, end?: boolean) => {
    if (to === '/app/issues' && teamOpen) return false;
    return end ? pathname === to || pathname.startsWith('/app/graphs') : pathname.startsWith(to);
  };
  return (
    <SidebarGroup className="py-1">
      {label && <SidebarGroupLabel className="h-7">{label}</SidebarGroupLabel>}
      <SidebarMenu className="gap-0.5">
        {items.map((item) => (
          <SidebarMenuItem key={item.to}>
            <SidebarMenuButton
              asChild
              tooltip={item.title}
              isActive={isActive(item.to, item.end)}
              className={ROW}
            >
              <NavLink to={item.to} end={item.end}>
                <item.icon />
                <span>{item.title}</span>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        ))}
      </SidebarMenu>
    </SidebarGroup>
  );
}

/** The teams the member belongs to, each a shortcut to that team's issues. */
function NavTeams() {
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
    <SidebarGroup className="py-1 group-data-[collapsible=icon]:hidden">
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

export function NavMain() {
  return (
    <>
      <NavSection items={WORK_ITEMS} />
      <NavTeams />
      <NavSection label="Build" items={BUILD_ITEMS} />
      <NavSection label="Workspace" items={ADMIN_ITEMS} />
      <SidebarGroup className="py-1">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild tooltip={API_DOCS_ITEM.title} className={ROW}>
              <a href={API_DOCS_ITEM.to} target="_blank" rel="noopener noreferrer">
                <API_DOCS_ITEM.icon />
                <span>{API_DOCS_ITEM.title}</span>
                <ArrowUpRightIcon className="ml-auto opacity-50" aria-hidden />
                <span className="sr-only">(opens in a new tab)</span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarGroup>
    </>
  );
}
