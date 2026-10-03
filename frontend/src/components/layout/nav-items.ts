import {
  BookOpenTextIcon,
  BotIcon,
  BrainIcon,
  type LucideIcon,
  NetworkIcon,
  PlayIcon,
  SettingsIcon,
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
  { title: 'Graphs', to: '/app', icon: NetworkIcon, end: true },
  { title: 'Runs', to: '/app/runs', icon: PlayIcon },
  { title: 'Agents', to: '/app/agents', icon: BotIcon },
  { title: 'Memory', to: '/app/memory', icon: BrainIcon },
  { title: 'Settings', to: '/app/settings', icon: SettingsIcon },
];

export const API_DOCS_ITEM: NavItem = {
  title: 'API docs',
  to: env.apiDocsUrl,
  icon: BookOpenTextIcon,
  external: true,
};
