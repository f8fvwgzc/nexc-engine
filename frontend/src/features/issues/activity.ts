import { useCallback, useState } from 'react';

import { githubLabel, githubRef } from '@/lib/github';
import type { IssueEvent } from '@/schemas/issue';

const PREFS_KEY = 'nexc.issue.activity';

/** Which entries of an issue's activity are shown. */
export type ActivityShow = 'all' | 'comments' | 'changes' | 'runs' | 'files';

export const ACTIVITY_SHOW_OPTIONS: { value: ActivityShow; label: string }[] = [
  { value: 'all', label: 'All activity' },
  { value: 'comments', label: 'Comments' },
  { value: 'changes', label: 'Changes' },
  { value: 'runs', label: 'Runs' },
  { value: 'files', label: 'Files' },
];

/** How a member likes an issue's activity: folded or open, what it shows, and in which order. */
export interface ActivityPrefs {
  collapsed: boolean;
  show: ActivityShow;
  /** `asc` oldest first, `desc` newest first. */
  order: 'asc' | 'desc';
}

export const DEFAULT_ACTIVITY_PREFS: ActivityPrefs = {
  collapsed: false,
  show: 'all',
  order: 'asc',
};

const isShow = (value: unknown): value is ActivityShow =>
  ACTIVITY_SHOW_OPTIONS.some((o) => o.value === value);

/**
 * The remembered preferences, the same on every issue. Anything missing, unreadable or from an
 * older shape falls back to the default for that field.
 */
export function readActivityPrefs(): ActivityPrefs {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(PREFS_KEY) ?? 'null');
    if (typeof raw !== 'object' || raw === null) return DEFAULT_ACTIVITY_PREFS;
    const stored = raw as Record<string, unknown>;
    return {
      collapsed: stored.collapsed === true,
      show: isShow(stored.show) ? stored.show : DEFAULT_ACTIVITY_PREFS.show,
      order: stored.order === 'desc' ? 'desc' : 'asc',
    };
  } catch {
    return DEFAULT_ACTIVITY_PREFS;
  }
}

export function writeActivityPrefs(prefs: ActivityPrefs) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // The choice lasts for this visit only.
  }
}

/** The activity preferences and a setter that changes some of them and remembers the result. */
export function useActivityPrefs(): [ActivityPrefs, (change: Partial<ActivityPrefs>) => void] {
  const [prefs, setPrefs] = useState(readActivityPrefs);
  const update = useCallback((change: Partial<ActivityPrefs>) => {
    setPrefs((current) => {
      const next = { ...current, ...change };
      writeActivityPrefs(next);
      return next;
    });
  }, []);
  return [prefs, update];
}

/** Whether an entry belongs to what the activity shows. */
export function showsEvent(event: IssueEvent, show: ActivityShow): boolean {
  switch (show) {
    case 'all':
      return true;
    case 'comments':
      return event.kind === 'comment';
    case 'runs':
      return event.kind === 'run';
    case 'files':
      return event.kind === 'artifact';
    case 'changes':
      return event.kind !== 'comment' && event.kind !== 'run' && event.kind !== 'artifact';
  }
}

/** What a file entry says: `to` is the action, `updated:v3` for a new version. */
function artifactText(name: string, action: string | null): string {
  if (action === 'uploaded') return `uploaded ${name}`;
  if (action === 'duplicated') return `duplicated ${name}`;
  if (action === 'deleted') return `deleted ${name}`;
  const version = action?.match(/^updated:v(\d+)$/)?.[1];
  if (version) return `updated ${name} to v${version}`;
  return `changed ${name}`;
}

/** What a change entry says after the actor's name. */
export function changeText(event: IssueEvent): string {
  const { from, to } = event;
  switch (event.kind) {
    case 'state':
      return `moved this from ${from ?? 'no state'} to ${to ?? 'no state'}`;
    case 'priority':
      return `set priority to ${to ?? 'none'}`;
    case 'assignee':
      if (!to) return `unassigned ${from ?? 'the assignee'}`;
      return `assigned this to ${to}`;
    case 'title':
      return `renamed this to “${to ?? ''}”`;
    case 'due':
      if (!to) return 'removed the due date';
      return from ? `moved the due date from ${from} to ${to}` : `set the due date to ${to}`;
    case 'comment':
      return 'commented';
    case 'run':
      return to === 'succeeded' ? 'finished the work' : 'ran the work';
    case 'github':
      if (!to) return 'removed the GitHub link';
      return `linked GitHub ${githubRef(to)?.kind === 'pull' ? 'pull request' : 'issue'} ${githubLabel(to)}`;
    case 'artifact':
      return artifactText(event.body || 'a file', to);
    case 'agent':
      // `body` is what a mention asked of the agent named in `to`.
      if (event.body === 'asked to plan') return `asked ${to ?? 'an agent'} to plan this`;
      return `delegated this to ${to ?? 'an agent'}`;
  }
}
