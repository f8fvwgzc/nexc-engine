import {
  BookOpenTextIcon,
  BotIcon,
  BrainIcon,
  ChartNoAxesColumnIcon,
  CircleDotIcon,
  FolderKanbanIcon,
  InboxIcon,
  type LucideIcon,
  NetworkIcon,
  PlayIcon,
  ScrollTextIcon,
  SettingsIcon,
  UsersIcon,
  UsersRoundIcon,
} from 'lucide-react';

import { env } from '@/lib/env';

export interface NavItem {
  title: string;
  to: string;
  icon: LucideIcon;
  /** Match only the exact path (the dashboard is the /app index). */
  end?: boolean;
  external?: boolean;
  /** Shown only to workspace owners and admins. */
  adminOnly?: boolean;
}

/** What the workspace is working on. */
export const WORK_ITEMS: NavItem[] = [
  { title: 'Inbox', to: '/app/inbox', icon: InboxIcon },
  { title: 'Issues', to: '/app/issues', icon: CircleDotIcon },
  { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
];

/** Where work is planned and executed by agents. */
export const BUILD_ITEMS: NavItem[] = [
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
];

/** The way into the settings area, which has its own sidebar. */
export const SETTINGS_ITEM: NavItem = {
  title: 'Settings',
  to: '/app/settings',
  icon: SettingsIcon,
};

/** Every page, for the command palette. */
export const NAV_ITEMS: NavItem[] = [
  { title: 'Inbox', to: '/app/inbox', icon: InboxIcon },
  { title: 'Issues', to: '/app/issues', icon: CircleDotIcon },
  { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
  { title: 'Memory', to: '/app/settings/memory', icon: BrainIcon },
  { title: 'Teams', to: '/app/settings/teams', icon: UsersRoundIcon },
  { title: 'Members', to: '/app/settings/members', icon: UsersIcon },
  { title: 'Usage', to: '/app/settings/usage', icon: ChartNoAxesColumnIcon },
  { title: 'Audit log', to: '/app/settings/audit', icon: ScrollTextIcon, adminOnly: true },
  { title: 'Settings', to: '/app/settings', icon: SettingsIcon },
];

export const API_DOCS_ITEM: NavItem = {
  title: 'API docs',
  to: env.apiDocsUrl,
  icon: BookOpenTextIcon,
  external: true,
};
