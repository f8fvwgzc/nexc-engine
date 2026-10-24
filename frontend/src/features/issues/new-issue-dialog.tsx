import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { StateGlyph } from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { githubUrlError } from '@/lib/github';
import { qk } from '@/lib/query-keys';
import { PRIORITY_LABEL } from '@/schemas/issue';
import type { Team, Workspace } from '@/schemas/workspace';

import { createIssue, projectsQuery, statesQuery } from './api';

const NONE = '__none__';

const PRIORITY_OPTIONS = PRIORITY_LABEL.map((label, value) => ({ value: String(value), label }));

/**
 * Files a new issue in a team. Opened from a board column it starts in that column's state, and
 * from a project it is filed in that project; both can still be changed before it is created.
 */
export function NewIssueDialog({
  workspace,
  teams,
  defaultTeam,
  defaultProject,
  defaultState,
  open,
  onClose,
}: {
  workspace: Workspace;
  teams: Team[];
  defaultTeam: string | undefined;
  defaultProject: string | undefined;
  /** A state of `defaultTeam`'s workflow; without one the issue starts in the first open state. */
  defaultState?: string;
  open: boolean;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const [teamId, setTeamId] = useState(defaultTeam ?? teams[0]?.id ?? '');
  const [stateId, setStateId] = useState(defaultState ?? NONE);
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('0');
  const [projectId, setProjectId] = useState(defaultProject ?? NONE);
  const [dueDate, setDueDate] = useState('');
  const [githubUrl, setGithubUrl] = useState('');
  const { data: projects = [] } = useQuery(projectsQuery(workspace.id));
  const { data: states = [] } = useQuery({
    ...statesQuery(workspace.id, teamId),
    enabled: open && teamId !== '',
  });
  const create = useMutation({
    mutationFn: () =>
      createIssue(workspace.id, teamId, {
        title: title.trim(),
        description,
        priority: Number(priority),
        state_id: stateId === NONE ? undefined : stateId,
        project_id: projectId === NONE ? undefined : projectId,
        due_date: dueDate || undefined,
        github_url: githubUrl.trim() || undefined,
      }),
    meta: { errorToast: false, successMessage: 'Issue created' },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.issues.all });
      setTitle('');
      setDescription('');
      setPriority('0');
      setDueDate('');
      setGithubUrl('');
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (title.trim() && teamId) create.mutate();
          }}
        >
          <DialogHeader>
            <DialogTitle>New issue</DialogTitle>
            <DialogDescription>
              It gets the team’s next number and starts in the state you pick, or the team’s first
              open state.
            </DialogDescription>
          </DialogHeader>
          <div className="grid grid-cols-2 gap-2">
            <OptionSelect
              value={teamId}
              onValueChange={(id) => {
                setTeamId(id);
                // A state belongs to one team's workflow.
                setStateId(NONE);
              }}
              options={teams.map((t) => ({ value: t.id, label: `${t.key} · ${t.name}` }))}
              aria-label="Team"
            />
            <OptionSelect
              value={stateId}
              onValueChange={setStateId}
              options={[
                { value: NONE, label: 'First open state' },
                ...states.map((s) => ({
                  value: s.id,
                  label: (
                    <span className="flex items-center gap-2">
                      <StateGlyph category={s.category} color={s.color} />
                      {s.name}
                    </span>
                  ),
                })),
              ]}
              aria-label="Status"
            />
            <OptionSelect
              value={priority}
              onValueChange={setPriority}
              options={PRIORITY_OPTIONS}
              aria-label="Priority"
            />
            <OptionSelect
              value={projectId}
              onValueChange={setProjectId}
              options={[
                { value: NONE, label: 'No project' },
                ...projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
              aria-label="Project"
            />
          </div>
          <Input
            autoFocus
            value={title}
            maxLength={200}
            placeholder="Issue title"
            aria-label="Title"
            onChange={(e) => setTitle(e.target.value)}
          />
          <Textarea
            value={description}
            rows={5}
            placeholder="What needs to happen, and why (optional). Agents work from it when the issue is handed to them."
            aria-label="Description"
            onChange={(e) => setDescription(e.target.value)}
          />
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[13px] text-muted-foreground">
            <label className="flex items-center gap-2">
              Due
              <Input
                type="date"
                value={dueDate}
                aria-label="Due date"
                className="h-8 w-40 text-[13px] text-foreground"
                onChange={(e) => setDueDate(e.target.value)}
              />
            </label>
            <label className="flex min-w-56 flex-1 items-center gap-2">
              GitHub
              <Input
                value={githubUrl}
                inputMode="url"
                placeholder="Issue or PR URL (optional)"
                aria-label="GitHub issue or PR URL"
                className="h-8 text-[13px] text-foreground"
                onChange={(e) => setGithubUrl(e.target.value)}
              />
            </label>
          </div>
          {create.error && (
            <p role="alert" className="text-sm text-destructive">
              {githubUrlError(create.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!title.trim() || !teamId || create.isPending}>
              Create issue
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
