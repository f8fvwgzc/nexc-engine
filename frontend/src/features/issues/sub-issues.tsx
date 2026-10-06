import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlusIcon } from 'lucide-react';
import { useState } from 'react';

import { PersonGlyph, StateGlyph } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Issue } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { createIssue, issuesQuery } from './api';

/**
 * The parts an issue is split into, with how many are closed, and a field to add one. A new part
 * is filed with the same team as the issue.
 */
export function SubIssues({
  workspace,
  issue,
  onOpen,
}: {
  workspace: Workspace;
  issue: Issue;
  onOpen: (issueId: string) => void;
}) {
  const queryClient = useQueryClient();
  const [title, setTitle] = useState('');
  const [adding, setAdding] = useState(false);
  const { data = [] } = useQuery({
    ...issuesQuery(workspace.id, { parent_id: issue.id }),
    enabled: issue.sub_issues.total > 0,
  });
  // In the order they were added, not by last change.
  const parts = [...data].sort((a, b) => a.created_at.localeCompare(b.created_at));
  const add = useMutation({
    mutationFn: () =>
      createIssue(workspace.id, issue.team_id, { title: title.trim(), parent_id: issue.id }),
    meta: { errorToast: false },
    onSuccess: () => {
      setTitle('');
      void queryClient.invalidateQueries({ queryKey: qk.issues.all });
    },
  });
  const { total, closed } = issue.sub_issues;

  return (
    <section aria-label="Sub-issues" className="space-y-2 border-t pt-4">
      <div className="flex items-center gap-2">
        <h3 className="text-[13px] font-medium text-muted-foreground">Sub-issues</h3>
        {total > 0 && (
          <span className="text-xs text-muted-foreground tabular-nums">
            {closed}/{total} closed
          </span>
        )}
        {!adding && (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto h-6 gap-1.5 px-2 text-xs"
            onClick={() => setAdding(true)}
          >
            <PlusIcon className="size-3.5" />
            Add sub-issue
          </Button>
        )}
      </div>
      {parts.length > 0 && (
        <ul className="divide-y rounded-md border">
          {parts.map((part) => (
            <li key={part.id}>
              <button
                type="button"
                onClick={() => onOpen(part.id)}
                className="flex h-8 w-full items-center gap-2.5 px-3 text-left text-[13px] outline-none hover:bg-muted/60 focus-visible:bg-muted/60"
              >
                <StateGlyph category={part.state.category} color={part.state.color} />
                <span className="shrink-0 font-mono text-xs text-muted-foreground">
                  {part.identifier}
                </span>
                <span className="min-w-0 flex-1 truncate">{part.title}</span>
                <PersonGlyph name={part.assignee?.name} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {adding && (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim()) add.mutate();
          }}
        >
          <Input
            autoFocus
            value={title}
            maxLength={200}
            placeholder="Sub-issue title"
            aria-label="Sub-issue title"
            className="h-8 text-[13px]"
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.stopPropagation();
                setAdding(false);
              }
            }}
          />
          <Button type="submit" size="sm" disabled={!title.trim() || add.isPending}>
            Add
          </Button>
        </form>
      )}
      {add.error && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(add.error)}
        </p>
      )}
    </section>
  );
}
