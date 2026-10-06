import type { ComponentProps } from 'react';

import { Sidebar, SidebarContent, SidebarFooter, SidebarHeader } from '@/components/ui/sidebar';

import { NavGraphs } from './nav-graphs';
import { NavMain } from './nav-main';
import { NavUser } from './nav-user';
import { WorkspaceSwitcher } from './workspace-switcher';

/** Collapsible icon sidebar; the page renders as an inset card beside it. */
export function AppSidebar(props: ComponentProps<typeof Sidebar>) {
  return (
    <Sidebar variant="inset" {...props}>
      <SidebarHeader>
        <WorkspaceSwitcher />
      </SidebarHeader>
      <SidebarContent>
        <NavMain />
        <NavGraphs />
      </SidebarContent>
      <SidebarFooter>
        <NavUser />
      </SidebarFooter>
    </Sidebar>
  );
}
