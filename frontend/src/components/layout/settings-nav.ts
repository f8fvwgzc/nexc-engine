import {
  BookOpenIcon,
  BrainIcon,
  CalendarDaysIcon,
  ChartNoAxesColumnIcon,
  DatabaseIcon,
  GavelIcon,
  KeyRoundIcon,
  type LucideIcon,
  PaletteIcon,
  PlugZapIcon,
  ScrollTextIcon,
  ShieldCheckIcon,
  SirenIcon,
  UserIcon,
  UsersIcon,
  UsersRoundIcon,
  WaypointsIcon,
} from 'lucide-react';

export interface SettingsItem {
  title: string;
  to: string;
  icon: LucideIcon;
  /** Shown only to workspace owners and admins. */
  adminOnly?: boolean;
}

export interface SettingsGroup {
  label: string;
  /** One line under the label saying what the group is for. */
  hint: string;
  items: SettingsItem[];
}

export const SETTINGS_ROOT = '/app/settings';

/**
 * The settings area, grouped by what a setting is about. "Workspace" is replaced by the open
 * workspace's name in the sidebar. What concerns the whole installation is not here: it is in
 * the platform console, for platform administrators.
 */
export const SETTINGS_GROUPS: SettingsGroup[] = [
  {
    label: 'Account',
    hint: 'You, on this device',
    items: [
      { title: 'Profile', to: `${SETTINGS_ROOT}/profile`, icon: UserIcon },
      { title: 'Preferences', to: `${SETTINGS_ROOT}/preferences`, icon: PaletteIcon },
    ],
  },
  {
    label: 'Workspace',
    hint: 'People and teams',
    items: [
      { title: 'Members', to: `${SETTINGS_ROOT}/members`, icon: UsersIcon },
      { title: 'Teams', to: `${SETTINGS_ROOT}/teams`, icon: UsersRoundIcon },
    ],
  },
  {
    label: 'AI',
    hint: 'Models, limits and what agents know',
    items: [
      { title: 'AI accounts', to: `${SETTINGS_ROOT}/ai`, icon: KeyRoundIcon },
      { title: 'Guardrails', to: `${SETTINGS_ROOT}/guardrails`, icon: ShieldCheckIcon },
      { title: 'Policies', to: `${SETTINGS_ROOT}/policies`, icon: GavelIcon, adminOnly: true },
      { title: 'Alerts', to: `${SETTINGS_ROOT}/alerts`, icon: SirenIcon, adminOnly: true },
      { title: 'MCP servers', to: `${SETTINGS_ROOT}/mcp-servers`, icon: PlugZapIcon },
      { title: 'Knowledge', to: `${SETTINGS_ROOT}/knowledge`, icon: BookOpenIcon },
      { title: 'Memory', to: `${SETTINGS_ROOT}/memory`, icon: BrainIcon },
    ],
  },
  {
    label: 'Insight',
    hint: 'What happened and what it cost',
    items: [
      {
        title: 'Activity',
        to: `${SETTINGS_ROOT}/activity`,
        icon: CalendarDaysIcon,
        adminOnly: true,
      },
      { title: 'Usage', to: `${SETTINGS_ROOT}/usage`, icon: ChartNoAxesColumnIcon },
      {
        title: 'Workspace map',
        to: `${SETTINGS_ROOT}/map`,
        icon: WaypointsIcon,
        adminOnly: true,
      },
      { title: 'Audit log', to: `${SETTINGS_ROOT}/audit`, icon: ScrollTextIcon, adminOnly: true },
    ],
  },
  {
    label: 'Data',
    hint: 'Take your data with you',
    items: [
      {
        title: 'Data transfer',
        to: `${SETTINGS_ROOT}/transfer`,
        icon: DatabaseIcon,
        adminOnly: true,
      },
    ],
  },
];
