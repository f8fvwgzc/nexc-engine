import { Link } from 'react-router-dom';

import { WithTooltip } from '@/components/custom-ui/with-tooltip';
import { prefetchRouteOnIntent } from '@/lib/prefetch';
import { cn } from '@/lib/utils';

import { BrandMark } from './brand-mark';
import type { CoreMenu } from './nav-items';
import { NavUser } from './nav-user';

interface ActivityRailProps {
  menus: CoreMenu[];
  /** The menu the open page belongs to. */
  active?: CoreMenu;
  /** Where the brand mark goes. */
  home: string;
  /** How much is unread behind each menu, by its id: a dot on the icon, the count in its name. */
  badges?: Record<string, number>;
}

function RailLink({ menu, active, count }: { menu: CoreMenu; active: boolean; count: number }) {
  return (
    <WithTooltip label={count > 0 ? `${menu.title} · ${count} unread` : menu.title} side="right">
      <Link
        to={menu.home}
        aria-current={active ? 'page' : undefined}
        data-active={active}
        data-rail-menu
        {...prefetchRouteOnIntent(menu.home)}
        className={cn(
          'relative flex size-9 items-center justify-center rounded-lg text-sidebar-foreground/70 outline-hidden transition-colors duration-100',
          'hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring',
          'data-[active=true]:bg-sidebar-accent data-[active=true]:text-sidebar-accent-foreground',
          // The mark of the open menu: a short bar on the rail's own edge.
          'before:absolute before:top-1/2 before:-left-1.5 before:h-4 before:w-0.5 before:-translate-y-1/2 before:rounded-full before:bg-sidebar-primary before:opacity-0 before:transition-opacity before:duration-100 data-[active=true]:before:opacity-100',
        )}
      >
        <menu.icon className="size-4.5" aria-hidden />
        {count > 0 && (
          <span
            aria-hidden
            data-slot="unread-dot"
            className="absolute top-1.5 right-1.5 size-2 rounded-full bg-primary ring-2 ring-sidebar"
          />
        )}
        <span className="sr-only">
          {menu.title}
          {count > 0 && ` (${count} unread)`}
        </span>
      </Link>
    </WithTooltip>
  );
}

/**
 * The rail: one icon per core menu, the brand mark above them and, at the bottom, the settings
 * and the account menu. It is always on screen; the panel beside it is what collapses.
 */
export function ActivityRail({ menus, active, home, badges = {} }: ActivityRailProps) {
  const link = (menu: CoreMenu) => (
    <li key={menu.id}>
      <RailLink menu={menu} active={menu.id === active?.id} count={badges[menu.id] ?? 0} />
    </li>
  );
  return (
    <nav
      aria-label="Sections"
      className="flex w-12 shrink-0 flex-col items-center gap-1 border-r border-sidebar-border py-2"
    >
      <Link
        to={home}
        aria-label="Nexc"
        className="mb-1 flex size-9 items-center justify-center rounded-lg outline-hidden focus-visible:ring-2 focus-visible:ring-sidebar-ring"
      >
        <BrandMark className="size-7" />
      </Link>
      <ul className="flex flex-col items-center gap-1">
        {menus.filter((menu) => !menu.edge).map(link)}
      </ul>
      <ul className="mt-auto flex flex-col items-center gap-1">
        {menus.filter((menu) => menu.edge).map(link)}
        <li>
          <NavUser />
        </li>
      </ul>
    </nav>
  );
}
