import { fixtures } from '../fixtures';

import type { Persona } from './mock-api';

export interface AppRoute {
  /** Short, file-name safe. */
  name: string;
  path: string;
  persona: Persona;
  /** Public pages are visited signed out. */
  signedOut?: boolean;
  /** A list page: with no data it must guide the reader to the first item. */
  list?: boolean;
  /** One of the main pages the UI suite keeps a screenshot baseline of. */
  baseline?: boolean;
  /** A text that shows once the page has its data (not in a skeleton). */
  ready?: string;
}

const graphId = fixtures.graphs[0]?.id ?? '';
const runId = fixtures.runs[0]?.id ?? '';
const projectId = fixtures.projects[0]?.id ?? '';

/**
 * Every route of `src/app/router.tsx` (redirects are visited at their target). The login and
 * register pages are left out: the auth flow is not this suite's to change.
 */
export const APP_ROUTES: AppRoute[] = [
  { name: 'landing', path: '/', persona: 'member', signedOut: true },
  { name: 'reset-password', path: '/reset-password', persona: 'member', signedOut: true },
  { name: 'dashboard', path: '/app', persona: 'member', list: true, baseline: true },
  { name: 'graph', path: `/app/graphs/${graphId}`, persona: 'member' },
  { name: 'runs', path: '/app/runs', persona: 'member', list: true, baseline: true },
  { name: 'run-detail', path: `/app/runs/${runId}`, persona: 'member', baseline: true },
  { name: 'agents', path: '/app/agents', persona: 'member', list: true, baseline: true },
  { name: 'secops', path: '/app/secops', persona: 'member', baseline: true },
  { name: 'sessions', path: '/app/sessions', persona: 'member', list: true, baseline: true },
  { name: 'session', path: `/app/sessions/${runId}`, persona: 'member' },
  { name: 'inbox', path: '/app/inbox', persona: 'member', list: true, baseline: true },
  { name: 'issues', path: '/app/issues', persona: 'member', list: true, baseline: true },
  { name: 'my-issues', path: '/app/my-issues', persona: 'member', list: true },
  { name: 'projects', path: '/app/projects', persona: 'member', list: true, baseline: true },
  { name: 'project', path: `/app/projects/${projectId}`, persona: 'member' },
  { name: 'conversations', path: '/app/conversations', persona: 'member', list: true },
  { name: 'app-not-found', path: '/app/no-such-page', persona: 'member' },
  { name: 'settings-profile', path: '/app/settings/profile', persona: 'member' },
  { name: 'settings-preferences', path: '/app/settings/preferences', persona: 'member' },
  {
    name: 'settings-members',
    path: '/app/settings/members',
    persona: 'member',
    list: true,
    baseline: true,
  },
  { name: 'settings-teams', path: '/app/settings/teams', persona: 'member', list: true },
  { name: 'settings-ai', path: '/app/settings/ai', persona: 'member' },
  { name: 'settings-guardrails', path: '/app/settings/guardrails', persona: 'member' },
  { name: 'settings-policies', path: '/app/settings/policies', persona: 'member', list: true },
  { name: 'settings-alerts', path: '/app/settings/alerts', persona: 'member', list: true },
  {
    name: 'settings-alert-destinations',
    path: '/app/settings/alert-destinations',
    persona: 'member',
    list: true,
  },
  {
    name: 'settings-mcp-servers',
    path: '/app/settings/mcp-servers',
    persona: 'member',
    list: true,
  },
  {
    name: 'settings-a2a-counterparts',
    path: '/app/settings/a2a-counterparts',
    persona: 'member',
    list: true,
  },
  { name: 'settings-memory', path: '/app/settings/memory', persona: 'member', list: true },
  { name: 'settings-knowledge', path: '/app/settings/knowledge', persona: 'member', list: true },
  { name: 'settings-usage', path: '/app/settings/usage', persona: 'member' },
  { name: 'settings-audit', path: '/app/settings/audit', persona: 'member', list: true },
  { name: 'settings-activity', path: '/app/settings/activity', persona: 'member', list: true },
  { name: 'settings-map', path: '/app/settings/map', persona: 'member' },
  { name: 'settings-transfer', path: '/app/settings/transfer', persona: 'member' },
  { name: 'platform-workspaces', path: '/app/platform/workspaces', persona: 'admin', list: true },
  { name: 'platform-users', path: '/app/platform/users', persona: 'admin', list: true },
  { name: 'platform-activity', path: '/app/platform/activity', persona: 'admin', list: true },
  { name: 'platform-account', path: '/app/platform/account', persona: 'admin' },
  { name: 'platform-infrastructure', path: '/app/platform/infrastructure', persona: 'admin' },
  { name: 'not-found', path: '/no-such-page', persona: 'member', signedOut: true },
];

export const WIDTHS = [375, 768, 1280] as const;
export type Width = (typeof WIDTHS)[number];
export const HEIGHT: Record<Width, number> = { 375: 812, 768: 1024, 1280: 800 };
