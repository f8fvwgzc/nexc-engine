import { CalendarIcon } from 'lucide-react';

import type { Issue } from '@/schemas/issue';

import { dueLabel, localToday } from './due';

/** When an issue is due; red once that day has passed and the work is still open. */
export function DueChip({ issue }: { issue: Pick<Issue, 'due_date' | 'state'> }) {
  if (!issue.due_date) return null;
  const open = !['completed', 'canceled'].includes(issue.state.category);
  const { text, late } = dueLabel(issue.due_date, localToday(), open);
  return (
    <span
      title={`Due ${issue.due_date}`}
      className={`flex shrink-0 items-center gap-1 text-xs tabular-nums ${late ? 'font-medium text-destructive' : 'text-muted-foreground'}`}
    >
      <CalendarIcon aria-hidden className="size-3" />
      {text}
    </span>
  );
}
