import { ArrowUpRightIcon } from 'lucide-react';
import { NavLink, useLocation } from 'react-router-dom';

import {
  SidebarGroup,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar';

import { API_DOCS_ITEM, NAV_ITEMS } from './nav-items';

export function NavMain() {
  const { pathname } = useLocation();
  const isActive = (to: string, end?: boolean) =>
    end ? pathname === to || pathname.startsWith('/app/graphs') : pathname.startsWith(to);

  return (
    <SidebarGroup>
      <SidebarGroupLabel>Workspace</SidebarGroupLabel>
      <SidebarMenu>
        {NAV_ITEMS.map((item) => (
          <SidebarMenuItem key={item.to}>
            <SidebarMenuButton asChild tooltip={item.title} isActive={isActive(item.to, item.end)}>
              <NavLink to={item.to} end={item.end}>
                <item.icon />
                <span>{item.title}</span>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        ))}
        <SidebarMenuItem>
          <SidebarMenuButton asChild tooltip={API_DOCS_ITEM.title}>
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
  );
}
