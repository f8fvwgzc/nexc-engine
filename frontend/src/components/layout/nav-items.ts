import {
  BookOpenIcon,
  BookOpenTextIcon,
  BotIcon,
  BrainIcon,
  Building2Icon,
  CalendarDaysIcon,
  ChartNoAxesColumnIcon,
  CircleDotIcon,
  CircleUserIcon,
  DatabaseIcon,
  FilesIcon,
  FolderKanbanIcon,
  FolderOpenIcon,
  GavelIcon,
  HandshakeIcon,
  HistoryIcon,
  InboxIcon,
  KeyRoundIcon,
  LayoutDashboardIcon,
  ListTreeIcon,
  type LucideIcon,
  MessagesSquareIcon,
  NetworkIcon,
  PaletteIcon,
  PlayIcon,
  PlugZapIcon,
  ScrollTextIcon,
  ServerIcon,
  SettingsIcon,
  ShapesIcon,
  ShieldCheckIcon,
  ShieldHalfIcon,
  SirenIcon,
  UserIcon,
  UsersIcon,
  UsersRoundIcon,
  WaypointsIcon,
  WebhookIcon,
} from 'lucide-react';

import { env } from '@/lib/env';

export interface NavItem {
  title: string;
  to: string;
  icon: LucideIcon;
  /** Match only the exact path (the graphs list is the /app index). */
  end?: boolean;
  external?: boolean;
  /** Shown only to workspace owners and admins. */
  adminOnly?: boolean;
}

export interface NavGroup {
  /** Above the rows. A menu's first group usually has none: the menu's title is its label. */
  label?: string;
  items: NavItem[];
}

/**
 * A core menu: one icon in the rail, and the pages behind it in the panel beside the rail. The
 * rail marks the menu the open page belongs to; choosing another opens its home page and the
 * panel changes to its rows.
 */
export interface CoreMenu {
  id: string;
  title: string;
  icon: LucideIcon;
  /** Where the rail's button goes. */
  home: string;
  groups: NavGroup[];
  /** At the bottom of the rail, apart from the work: settings. */
  edge?: boolean;
}

const SETTINGS = '/app/settings';

/**
 * The workspace's core menus: what needs you and the issues agents work on, the files kept with
 * that work, the agents and the proof of what they did, the graphs they run, the trust layer over
 * all of it, and the settings. Trust pages live at settings addresses; they are listed where they
 * belong.
 */
export const WORKSPACE_MENUS: CoreMenu[] = [
  {
    id: 'issues',
    title: 'Issues',
    icon: CircleDotIcon,
    home: '/app/inbox',
    groups: [
      {
        items: [
          { title: 'Inbox', to: '/app/inbox', icon: InboxIcon },
          { title: 'My issues', to: '/app/my-issues', icon: CircleUserIcon },
          { title: 'All issues', to: '/app/issues', icon: CircleDotIcon },
          { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
        ],
      },
    ],
  },
  {
    id: 'artifacts',
    title: 'Artifacts',
    icon: FolderOpenIcon,
    home: '/app/artifacts',
    groups: [{ items: [{ title: 'All files', to: '/app/artifacts', icon: FilesIcon }] }],
  },
  {
    id: 'agents',
    title: 'Agents',
    icon: BotIcon,
    home: '/app/agents',
    groups: [
      {
        items: [
          { title: 'Agents', to: '/app/agents', icon: BotIcon },
          { title: 'Conversations', to: '/app/conversations', icon: MessagesSquareIcon },
          { title: 'Sessions', to: '/app/sessions', icon: ListTreeIcon },
          { title: 'Runs', to: '/app/runs', icon: PlayIcon },
        ],
      },
    ],
  },
  {
    id: 'graphs',
    title: 'Graphs',
    icon: NetworkIcon,
    home: '/app',
    groups: [{ items: [{ title: 'All graphs', to: '/app', icon: NetworkIcon, end: true }] }],
  },
  {
    id: 'trust',
    title: 'Trust',
    icon: ShieldCheckIcon,
    home: '/app/secops',
    groups: [
      {
        items: [
          { title: 'Overview', to: '/app/secops', icon: ShieldCheckIcon },
          { title: 'Guardrails', to: `${SETTINGS}/guardrails`, icon: ShieldHalfIcon },
          { title: 'Policies', to: `${SETTINGS}/policies`, icon: GavelIcon, adminOnly: true },
        ],
      },
      {
        label: 'Alerting',
        items: [
          { title: 'Alerts', to: `${SETTINGS}/alerts`, icon: SirenIcon, adminOnly: true },
          {
            title: 'Destinations',
            to: `${SETTINGS}/alert-destinations`,
            icon: WebhookIcon,
            adminOnly: true,
          },
        ],
      },
      {
        label: 'Evidence',
        items: [
          { title: 'Audit log', to: `${SETTINGS}/audit`, icon: ScrollTextIcon, adminOnly: true },
        ],
      },
    ],
  },
  {
    id: 'settings',
    title: 'Settings',
    icon: SettingsIcon,
    home: `${SETTINGS}/profile`,
    edge: true,
    groups: [
      {
        label: 'Account',
        items: [
          { title: 'Profile', to: `${SETTINGS}/profile`, icon: UserIcon },
          { title: 'Preferences', to: `${SETTINGS}/preferences`, icon: PaletteIcon },
        ],
      },
      {
        label: 'Workspace',
        items: [
          { title: 'Members', to: `${SETTINGS}/members`, icon: UsersIcon },
          { title: 'Teams', to: `${SETTINGS}/teams`, icon: UsersRoundIcon },
        ],
      },
      {
        label: 'AI',
        items: [
          { title: 'AI accounts', to: `${SETTINGS}/ai`, icon: KeyRoundIcon },
          { title: 'MCP servers', to: `${SETTINGS}/mcp-servers`, icon: PlugZapIcon },
          { title: 'A2A counterparts', to: `${SETTINGS}/a2a-counterparts`, icon: HandshakeIcon },
          { title: 'Knowledge', to: `${SETTINGS}/knowledge`, icon: BookOpenIcon },
          { title: 'Memory', to: `${SETTINGS}/memory`, icon: BrainIcon },
        ],
      },
      {
        label: 'Insight',
        items: [
          {
            title: 'Activity',
            to: `${SETTINGS}/activity`,
            icon: CalendarDaysIcon,
            adminOnly: true,
          },
          { title: 'Usage', to: `${SETTINGS}/usage`, icon: ChartNoAxesColumnIcon },
          { title: 'Workspace map', to: `${SETTINGS}/map`, icon: WaypointsIcon },
          { title: 'Ontology', to: `${SETTINGS}/ontology`, icon: ShapesIcon },
        ],
      },
      {
        label: 'Data',
        items: [
          {
            title: 'Data transfer',
            to: `${SETTINGS}/transfer`,
            icon: DatabaseIcon,
            adminOnly: true,
          },
        ],
      },
    ],
  },
];

/** The platform console's one menu: the whole installation, for platform administrators. */
export const PLATFORM_MENUS: CoreMenu[] = [
  {
    id: 'console',
    title: 'Console',
    icon: LayoutDashboardIcon,
    home: '/app/platform/workspaces',
    groups: [
      {
        label: 'Tenants',
        items: [
          { title: 'Workspaces', to: '/app/platform/workspaces', icon: Building2Icon },
          { title: 'Accounts', to: '/app/platform/users', icon: UsersIcon },
        ],
      },
      {
        label: 'Oversight',
        items: [{ title: 'Activity', to: '/app/platform/activity', icon: HistoryIcon }],
      },
      {
        label: 'Server',
        items: [{ title: 'Infrastructure', to: '/app/platform/infrastructure', icon: ServerIcon }],
      },
      {
        label: 'You',
        items: [{ title: 'Account', to: '/app/platform/account', icon: UserIcon }],
      },
    ],
  },
];

/** Whether the page at `pathname` is the item's page or one under it. */
export function itemMatches(item: NavItem, pathname: string): boolean {
  // The graphs list is the /app index, and each graph's canvas is under /app/graphs.
  if (item.end) return pathname === item.to || pathname.startsWith('/app/graphs/');
  return pathname === item.to || pathname.startsWith(`${item.to}/`);
}

/** The menu the page at `pathname` belongs to: the one with the most specific matching row. */
export function activeMenu(menus: CoreMenu[], pathname: string): CoreMenu | undefined {
  let best: { menu: CoreMenu; length: number } | undefined;
  for (const menu of menus) {
    for (const item of menu.groups.flatMap((group) => group.items)) {
      if (itemMatches(item, pathname) && (!best || item.to.length > best.length)) {
        best = { menu, length: item.to.length };
      }
    }
  }
  return best?.menu;
}

/** Every page, for the command palette, named by its menu where the row's name alone says less. */
export const NAV_ITEMS: NavItem[] = WORKSPACE_MENUS.flatMap((menu) =>
  menu.groups.flatMap((group) =>
    group.items.map((item) => ({
      ...item,
      title: item.title === menu.title ? item.title : `${menu.title} › ${item.title}`,
    })),
  ),
);

export const API_DOCS_ITEM: NavItem = {
  title: 'API docs',
  to: env.apiDocsUrl,
  icon: BookOpenTextIcon,
  external: true,
};

/** The documentation site, built from the repository's docs and published with each release. */
export const DOCS_ITEM: NavItem = {
  title: 'Documentation',
  to: 'https://f8fvwgzc.github.io/nexc-engine/',
  icon: BookOpenTextIcon,
  external: true,
};
