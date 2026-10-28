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

/**
 * Whether an entry reads as one short line: a change to the issue, or a file that is gone (there
 * is nothing left to open). Comments, run reports and files are entries of their own.
 */
export function isCompact(event: IssueEvent): boolean {
  if (event.kind === 'artifact') return event.to === 'deleted';
  return event.kind !== 'comment' && event.kind !== 'run';
}

/** How far apart two changes by the same person may be and still read as one entry. */
export const CHANGE_GROUP_MS = 5 * 60_000;

/** One entry of the activity as it is drawn. */
export type ActivityItem =
  | { kind: 'event'; event: IssueEvent }
  /** Changes made one after another by the same person, drawn as one entry of short lines. */
  | { kind: 'changes'; events: IssueEvent[] };

/**
 * The activity as it is drawn, in the order given: each comment, run report and file an entry of
 * its own, and changes that follow one another by the same person within a few minutes gathered
 * into one, as Linear does. `who` keys whose a change is (equal only for the same person).
 */
export function groupActivity(
  events: IssueEvent[],
  who: (event: IssueEvent) => string,
): ActivityItem[] {
  const items: ActivityItem[] = [];
  for (const event of events) {
    if (!isCompact(event)) {
      items.push({ kind: 'event', event });
      continue;
    }
    const last = items.at(-1);
    const previous = last?.kind === 'changes' ? last.events.at(-1) : undefined;
    const gap = previous
      ? Math.abs(Date.parse(event.created_at) - Date.parse(previous.created_at))
      : Infinity;
    if (
      last?.kind === 'changes' &&
      previous &&
      who(previous) === who(event) &&
      gap <= CHANGE_GROUP_MS
    )
      last.events.push(event);
    else items.push({ kind: 'changes', events: [event] });
  }
  return items;
}

/**
 * The entries of the activity, and the replies under each comment: a reply goes into its
 * comment's thread, oldest first whatever the activity's order, unless that comment is not among
 * the entries (then it stands on its own).
 */
export function threadReplies(events: IssueEvent[]): {
  entries: IssueEvent[];
  replies: Map<string, IssueEvent[]>;
} {
  const parents = new Set(events.filter((e) => e.parent_id === null).map((e) => e.id));
  const entries: IssueEvent[] = [];
  const replies = new Map<string, IssueEvent[]>();
  for (const event of events) {
    if (event.parent_id && parents.has(event.parent_id)) {
      replies.set(event.parent_id, [...(replies.get(event.parent_id) ?? []), event]);
    } else {
      entries.push(event);
    }
  }
  return { entries, replies };
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
