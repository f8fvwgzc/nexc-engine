import { MonitorIcon, MoonIcon, SunIcon, type LucideIcon } from 'lucide-react';

import type { Theme } from '@/app/providers/theme-context';

export const THEME_OPTIONS: { value: Theme; label: string; icon: LucideIcon }[] = [
  { value: 'light', label: 'Light', icon: SunIcon },
  { value: 'dark', label: 'Dark', icon: MoonIcon },
  { value: 'system', label: 'System', icon: MonitorIcon },
];

export function isTheme(value: string): value is Theme {
  return value === 'light' || value === 'dark' || value === 'system';
}
