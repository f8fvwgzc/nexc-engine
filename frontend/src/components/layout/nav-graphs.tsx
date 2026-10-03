import { useQuery, useQueryClient } from '@tanstack/react-query';
import { NetworkIcon, PlusIcon } from 'lucide-react';
import { Link, useParams } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
} from '@/components/ui/sidebar';
import { routeModules } from '@/app/route-modules';
import { graphQuery, graphsQuery } from '@/features/graphs/api';

const MAX_GRAPHS = 8;

/** Recent graphs; hovering/focusing one prefetches its data and the canvas chunk. */
export function NavGraphs() {
  const queryClient = useQueryClient();
  const { graphId: activeId } = useParams();
  const { data: graphs, isPending } = useQuery({
    ...graphsQuery(),
    select: (list) =>
      [...list].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, MAX_GRAPHS),
  });

  const prefetch = (graphId: string) => {
    void queryClient.prefetchQuery(graphQuery(graphId));
    void routeModules.graph();
  };

  return (
    <SidebarGroup className="group-data-[collapsible=icon]:hidden">
      <SidebarGroupLabel>Recent graphs</SidebarGroupLabel>
      <SidebarGroupAction asChild title="New graph">
        <Link to="/app?new=1" aria-label="Create a new graph">
          <PlusIcon />
        </Link>
      </SidebarGroupAction>
      <SidebarMenu>
        {isPending &&
          Array.from({ length: 3 }, (_, i) => (
            <SidebarMenuItem key={i}>
              <SidebarMenuSkeleton showIcon />
            </SidebarMenuItem>
          ))}
        {graphs?.map((graph) => (
          <SidebarMenuItem key={graph.id}>
            <SidebarMenuButton asChild isActive={graph.id === activeId} size="sm">
              <Link
                to={`/app/graphs/${graph.id}`}
                onMouseEnter={() => prefetch(graph.id)}
                onFocus={() => prefetch(graph.id)}
              >
                <NetworkIcon className="text-muted-foreground" />
                <span>{graph.name}</span>
              </Link>
            </SidebarMenuButton>
            <SidebarMenuBadge>{graph.node_count}</SidebarMenuBadge>
          </SidebarMenuItem>
        ))}
        {graphs?.length === 0 && (
          <p className="px-2 py-1.5 text-xs text-muted-foreground">No graphs yet.</p>
        )}
      </SidebarMenu>
    </SidebarGroup>
  );
}
