import {
  BookOpenIcon,
  DatabaseIcon,
  CalendarDaysIcon,
  ServerIcon,
  WaypointsIcon,
  BrainIcon,
  ChartNoAxesColumnIcon,
  KeyRoundIcon,
  type LucideIcon,
  PaletteIcon,
  ScrollTextIcon,
  ShieldCheckIcon,
  UserIcon,
  UsersIcon,
  UsersRoundIcon,
} from 'lucide-react';

export interface SettingsItem {
  title: string;
  to: string;
  icon: LucideIcon;
  /** Shown only to workspace owners and admins. */
  adminOnly?: boolean;
}

export const SETTINGS_ROOT = '/app/settings';

/** The settings area, grouped as its sidebar shows it. */
export const SETTINGS_GROUPS: { label: string; items: SettingsItem[] }[] = [
  {
    label: 'Account',
    items: [
      { title: 'Profile', to: `${SETTINGS_ROOT}/profile`, icon: UserIcon },
      { title: 'Preferences', to: `${SETTINGS_ROOT}/preferences`, icon: PaletteIcon },
    ],
  },
  {
    label: 'Workspace',
    items: [
      { title: 'Members', to: `${SETTINGS_ROOT}/members`, icon: UsersIcon },
      { title: 'Teams', to: `${SETTINGS_ROOT}/teams`, icon: UsersRoundIcon },
      { title: 'AI accounts', to: `${SETTINGS_ROOT}/ai`, icon: KeyRoundIcon },
      { title: 'Guardrails', to: `${SETTINGS_ROOT}/guardrails`, icon: ShieldCheckIcon },
      { title: 'Memory', to: `${SETTINGS_ROOT}/memory`, icon: BrainIcon },
      { title: 'Knowledge', to: `${SETTINGS_ROOT}/knowledge`, icon: BookOpenIcon },
      { title: 'Usage', to: `${SETTINGS_ROOT}/usage`, icon: ChartNoAxesColumnIcon },
      {
        title: 'Activity',
        to: `${SETTINGS_ROOT}/activity`,
        icon: CalendarDaysIcon,
        adminOnly: true,
      },
      {
        title: 'Workspace map',
        to: `${SETTINGS_ROOT}/map`,
        icon: WaypointsIcon,
        adminOnly: true,
      },
      {
        title: 'Audit log',
        to: `${SETTINGS_ROOT}/audit`,
        icon: ScrollTextIcon,
        adminOnly: true,
      },
      { title: 'Data transfer', to: `${SETTINGS_ROOT}/transfer`, icon: DatabaseIcon },
    ],
  },
  {
    label: 'Server',
    items: [{ title: 'Infrastructure', to: `${SETTINGS_ROOT}/infrastructure`, icon: ServerIcon }],
  },
];
