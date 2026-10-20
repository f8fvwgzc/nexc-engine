import {
  BookOpenTextIcon,
  BotIcon,
  BrainIcon,
  ChartNoAxesColumnIcon,
  CircleDotIcon,
  CircleUserIcon,
  FolderKanbanIcon,
  GavelIcon,
  InboxIcon,
  ListTreeIcon,
  type LucideIcon,
  MessagesSquareIcon,
  NetworkIcon,
  PlayIcon,
  ScrollTextIcon,
  SettingsIcon,
  ShieldCheckIcon,
  SirenIcon,
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

/**
 * The four places a workspace lives in: what needs you, the issues agents work on, the agents
 * themselves, and the proof of what they did.
 */
export const PRIMARY_ITEMS: NavItem[] = [
  { title: 'Inbox', to: '/app/inbox', icon: InboxIcon },
  { title: 'Issues', to: '/app/issues', icon: CircleDotIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
  { title: 'Trust', to: '/app/secops', icon: ShieldCheckIcon },
];

/** Everything else, one click away under "More": the views behind the issues. */
export const MORE_ITEMS: NavItem[] = [
  { title: 'My issues', to: '/app/my-issues', icon: CircleUserIcon },
  { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
  { title: 'Conversations', to: '/app/conversations', icon: MessagesSquareIcon },
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Sessions', to: '/app/sessions', icon: ListTreeIcon },
];

/** Every page, for the command palette. */
export const NAV_ITEMS: NavItem[] = [
  { title: 'Inbox', to: '/app/inbox', icon: InboxIcon },
  { title: 'My issues', to: '/app/my-issues', icon: CircleUserIcon },
  { title: 'Issues', to: '/app/issues', icon: CircleDotIcon },
  { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
  { title: 'Conversations', to: '/app/conversations', icon: MessagesSquareIcon },
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
  { title: 'Trust', to: '/app/secops', icon: ShieldCheckIcon },
  { title: 'Memory', to: '/app/settings/memory', icon: BrainIcon },
  { title: 'Teams', to: '/app/settings/teams', icon: UsersRoundIcon },
  { title: 'Members', to: '/app/settings/members', icon: UsersIcon },
  { title: 'Usage', to: '/app/settings/usage', icon: ChartNoAxesColumnIcon },
  { title: 'Audit log', to: '/app/settings/audit', icon: ScrollTextIcon, adminOnly: true },
  { title: 'Policies', to: '/app/settings/policies', icon: GavelIcon, adminOnly: true },
  { title: 'Alerts', to: '/app/settings/alerts', icon: SirenIcon, adminOnly: true },
  { title: 'Settings', to: '/app/settings', icon: SettingsIcon },
];

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
