import { useQuery } from '@tanstack/react-query';
import { ChevronRightIcon, CircleDotIcon, FolderKanbanIcon } from 'lucide-react';
import { Link, useLocation } from 'react-router-dom';

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from '@/components/ui/sidebar';
import { teamsQuery } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { prefetchRouteOnIntent } from '@/lib/prefetch';

import { ROW } from './nav-section';
import { useTeamExpansion } from './team-expansion';

/** The pages of one team: its issues and its projects, each filtered to the team. */
const TEAM_PAGES = [
  { title: 'Issues', path: '/app/issues', icon: CircleDotIcon },
  { title: 'Projects', path: '/app/projects', icon: FolderKanbanIcon },
] as const;

/**
 * The teams the member belongs to. Each one folds open, as in Linear, into its issues and its
 * projects; which teams are folded is remembered. A folded team is highlighted while one of its
 * pages is open.
 */
export function NavTeams() {
  const { current } = useCurrentWorkspace();
  const { search, pathname } = useLocation();
  const [isOpen, setOpen] = useTeamExpansion();
  const { data: teams = [] } = useQuery({
    ...teamsQuery(current?.id ?? ''),
    enabled: Boolean(current),
  });
  const mine = teams.filter((team) => team.role !== null);
  if (mine.length === 0) return null;
  const openTeam = new URLSearchParams(search).get('team');
  return (
    <SidebarGroup className="py-1">
      <SidebarGroupLabel className="h-7">Your teams</SidebarGroupLabel>
      <SidebarMenu className="gap-0.5">
        {mine.map((team) => {
          const open = isOpen(team.id);
          const shown = openTeam === team.id ? pathname : null;
          return (
            <Collapsible
              key={team.id}
              asChild
              open={open}
              onOpenChange={(next) => setOpen(team.id, next)}
            >
              <SidebarMenuItem>
                <CollapsibleTrigger asChild>
                  <SidebarMenuButton
                    isActive={!open && TEAM_PAGES.some((page) => page.path === shown)}
                    className={ROW}
                  >
                    <span className="flex size-4 shrink-0 items-center justify-center rounded-[4px] bg-foreground/10 text-[9px] font-semibold">
                      {team.key.slice(0, 1)}
                    </span>
                    <span className="min-w-0 truncate">{team.name}</span>
                    <ChevronRightIcon
                      aria-hidden
                      className="size-3.5! text-muted-foreground transition-transform group-data-[state=open]/menu-button:rotate-90"
                    />
                  </SidebarMenuButton>
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <SidebarMenuSub className="gap-0.5">
                    {TEAM_PAGES.map((page) => {
                      const active = page.path === shown;
                      return (
                        <SidebarMenuSubItem key={page.path}>
                          <SidebarMenuSubButton
                            asChild
                            isActive={active}
                            className="data-[size=md]:text-[13px]"
                          >
                            <Link
                              to={`${page.path}?team=${team.id}`}
                              aria-current={active ? 'page' : undefined}
                              {...prefetchRouteOnIntent(page.path)}
                            >
                              <page.icon />
                              <span>
                                {page.title}
                                <span className="sr-only">, {team.name}</span>
                              </span>
                            </Link>
                          </SidebarMenuSubButton>
                        </SidebarMenuSubItem>
                      );
                    })}
                  </SidebarMenuSub>
                </CollapsibleContent>
              </SidebarMenuItem>
            </Collapsible>
          );
        })}
      </SidebarMenu>
    </SidebarGroup>
  );
}
