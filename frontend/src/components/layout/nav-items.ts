import {
  BookOpenTextIcon,
  BotIcon,
  BrainIcon,
  ChartNoAxesColumnIcon,
  CircleDotIcon,
  FolderKanbanIcon,
  type LucideIcon,
  NetworkIcon,
  PlayIcon,
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
}

export const NAV_ITEMS: NavItem[] = [
  { title: 'Issues', to: '/app/issues', icon: CircleDotIcon },
  { title: 'Projects', to: '/app/projects', icon: FolderKanbanIcon },
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
  { title: 'Memory', to: '/app/memory', icon: BrainIcon },
  { title: 'Teams', to: '/app/teams', icon: UsersRoundIcon },
  { title: 'Members', to: '/app/members', icon: UsersIcon },
  { title: 'Usage', to: '/app/usage', icon: ChartNoAxesColumnIcon },
  { title: 'Settings', to: '/app/settings', icon: SettingsIcon },
];

export const API_DOCS_ITEM: NavItem = {
  title: 'API docs',
  to: env.apiDocsUrl,
  icon: BookOpenTextIcon,
  external: true,
};
