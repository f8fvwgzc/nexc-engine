import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronRightIcon, NetworkIcon, Trash2Icon, XIcon } from 'lucide-react';
import { useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { ConfirmDialog } from '@/components/custom-ui/confirm-dialog';
import { CopyButton } from '@/components/custom-ui/copy-button';
import { GithubChip } from '@/components/custom-ui/github-chip';
import { IconButton } from '@/components/custom-ui/icon-button';
import { PriorityGlyph, StateGlyph } from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { OntologyLink } from '@/features/ontology/ontology-link';
import { membersQuery, teamsQuery } from '@/features/workspaces/api';
import { errorMessage } from '@/lib/api/errors';
import { formatRelative } from '@/lib/format';
import { githubUrlError } from '@/lib/github';
import { qk } from '@/lib/query-keys';
import { cycleLabel } from '@/schemas/cycle';
import { PRIORITY_LABEL, type Issue, type IssueInput } from '@/schemas/issue';
import { ontoEntity } from '@/schemas/ontology';
import type { Workspace } from '@/schemas/workspace';

import { cyclesQuery, deleteIssue, projectsQuery, statesQuery, updateIssue } from './api';
import { IssueRun } from './issue-run';
import { IssueTimeline } from './issue-timeline';
import { LabelPicker } from './label-picker';
import { issuePath, useLeaveIssue } from './links';
import { SubIssues } from './sub-issues';

const NONE = '__none__';

/** One property of an issue in the side panel: its name beside its control. */
function Property({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-2">
      <p className="flex min-h-8 items-center text-xs text-muted-foreground">{label}</p>
      <div className="flex min-h-8 min-w-0 items-center">{children}</div>
    </div>
  );
}

/**
 * The GitHub issue or pull request the issue is linked to, with a button that removes the link;
 * without one, a field to paste a URL, saved when it is left or on Enter. A URL the server
 * refuses stays in the field with the server's reason under it.
 */
function GithubLink({ issue }: { issue: Issue }) {
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState('');
  const link = useMutation({
    mutationFn: (github_url: string | null) => updateIssue(issue.id, { github_url }),
    meta: { errorToast: false },
    onSuccess: (updated) => {
      setDraft('');
      queryClient.setQueryData(qk.issues.one(updated.id), updated);
      void queryClient.invalidateQueries({ queryKey: qk.issues.all });
    },
  });
  if (issue.github_url) {
    return (
      <div className="flex min-w-0 items-center gap-1">
        <GithubChip url={issue.github_url} form="repo" className="min-w-0 shrink" />
        <IconButton
          label="Remove the GitHub link"
          className="size-6 text-muted-foreground"
          disabled={link.isPending}
          onClick={() => link.mutate(null)}
        >
          <XIcon />
        </IconButton>
      </div>
    );
  }
  const commit = () => {
    const url = draft.trim();
    // The same refused URL is not sent again until it is changed.
    if (!url || link.isPending || (link.isError && link.variables === url)) return;
    link.mutate(url);
  };
  return (
    <div className="w-full space-y-1">
      <Input
        value={draft}
        inputMode="url"
        placeholder="Paste an issue or PR URL"
        aria-label="GitHub issue or PR URL"
        aria-invalid={link.isError || undefined}
        className="h-8 text-[13px]"
        onChange={(e) => {
          setDraft(e.target.value);
          if (link.isError) link.reset();
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          }
        }}
      />
      {link.error && (
        <p role="alert" className="text-xs text-destructive">
          {githubUrlError(link.error)}
        </p>
      )}
    </div>
  );
}

/**
 * Everything about one issue, editable in place, laid out like Linear: a main column that scrolls
 * (title, description, the agents' work, sub-issues, files, activity) beside a properties panel
 * with its own scroll; on a phone the panel follows the main column. It fills the height its page
 * gives it (`flex-1` in a flex column) from `md` up.
 */
export function IssueDetail({
  workspace,
  issue,
  artifacts,
}: {
  workspace: Workspace;
  issue: Issue;
  /** The issue's files, shown between its sub-issues and its activity. */
  artifacts?: ReactNode;
}) {
  const queryClient = useQueryClient();
  const { leave } = useLeaveIssue();
  const [title, setTitle] = useState(issue.title);
  const [description, setDescription] = useState(issue.description);
  const [confirming, setConfirming] = useState(false);
  const { data: states = [] } = useQuery(statesQuery(workspace.id, issue.team_id));
  const { data: projects = [] } = useQuery(projectsQuery(workspace.id));
  const { data: cycles = [] } = useQuery(cyclesQuery(workspace.id, issue.team_id));
  const { data: teams = [] } = useQuery(teamsQuery(workspace.id));
  // Guests cannot list the workspace's members; they keep whoever is assigned.
  const { data: members = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: workspace.role !== 'guest',
  });
  const team = teams.find((t) => t.id === issue.team_id);
  const save = useMutation({
    mutationFn: (body: Partial<IssueInput>) => updateIssue(issue.id, body),
    onSuccess: (updated) => {
      queryClient.setQueryData(qk.issues.one(updated.id), updated);
      void queryClient.invalidateQueries({ queryKey: qk.issues.all });
    },
  });
  const remove = useMutation({
    mutationFn: () => deleteIssue(issue.id),
    meta: { successMessage: 'Issue deleted' },
    onSuccess: () => {
      leave();
      // Everything else about issues is read again; the deleted issue's own entries just go.
      void queryClient.invalidateQueries({
        queryKey: qk.issues.all,
        predicate: (query) => !query.queryKey.includes(issue.id),
      });
    },
  });
  // Text saves when the field is left, like every other property saves when it is picked.
  const commitText = () => {
    const next = title.trim();
    if (!next) {
      setTitle(issue.title);
      return;
    }
    if (next !== issue.title || description !== issue.description) {
      save.mutate({ title: next, description });
    }
  };

  return (
    <div className="flex flex-col md:min-h-0 md:flex-1">
      <h1 className="sr-only">
        {issue.identifier} {issue.title}
      </h1>
      <div className="flex h-11 shrink-0 items-center gap-2 border-b px-4 sm:px-6">
        <nav
          aria-label="Where the issue belongs"
          className="flex min-w-0 flex-1 items-center gap-1.5 text-[13px] text-muted-foreground"
        >
          <Link
            to={`/app/issues?team=${issue.team_id}`}
            className="truncate underline-offset-2 hover:text-foreground hover:underline"
          >
            {team?.name ?? 'Team'}
          </Link>
          <ChevronRightIcon className="size-3.5 shrink-0" aria-hidden />
          {issue.parent && (
            <>
              <Link
                to={issuePath(issue.parent.id)}
                className="truncate underline-offset-2 hover:text-foreground hover:underline"
              >
                {issue.parent.identifier} {issue.parent.title}
              </Link>
              <ChevronRightIcon className="size-3.5 shrink-0" aria-hidden />
            </>
          )}
          <span className="shrink-0 font-mono text-xs text-foreground">{issue.identifier}</span>
        </nav>
        {issue.graph_id && (
          <Button variant="outline" size="sm" asChild>
            <Link to={`/app/graphs/${issue.graph_id}`}>
              <NetworkIcon />
              Open graph
            </Link>
          </Button>
        )}
        <OntologyLink entity={ontoEntity('issue', issue.id)} compact />
        <CopyButton value={`${window.location.origin}${issuePath(issue.id)}`} label="Copy link" />
        <IconButton
          className="text-muted-foreground hover:text-destructive"
          label={`Delete ${issue.identifier}`}
          onClick={() => setConfirming(true)}
        >
          <Trash2Icon />
        </IconButton>
      </div>
      <div className="flex flex-col md:min-h-0 md:flex-1 md:flex-row">
        <div className="min-w-0 flex-1 md:overflow-y-auto">
          <div className="mx-auto w-full max-w-[52rem] space-y-4 px-4 py-6 sm:px-8">
            <textarea
              value={title}
              rows={1}
              maxLength={200}
              aria-label="Title"
              placeholder="Issue title"
              className="field-sizing-content w-full resize-none bg-transparent text-xl font-semibold tracking-tight outline-none placeholder:text-muted-foreground"
              onChange={(e) => setTitle(e.target.value)}
              onBlur={commitText}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  e.currentTarget.blur();
                }
              }}
            />
            <Textarea
              value={description}
              aria-label="Description"
              placeholder="Add a description. Agents work from it when the issue is handed to them."
              className="min-h-24 resize-none rounded-none border-0 bg-transparent px-0 shadow-none focus-visible:ring-0 dark:bg-transparent"
              onChange={(e) => setDescription(e.target.value)}
              onBlur={commitText}
            />
            {issue.github_url && (
              <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
                Linked to <GithubChip url={issue.github_url} />
              </p>
            )}
            {save.error && (
              <p role="alert" className="text-sm text-destructive">
                {errorMessage(save.error)}
              </p>
            )}
            <IssueRun issue={issue} />
            <SubIssues workspace={workspace} issue={issue} />
            {artifacts}
            <IssueTimeline
              issue={issue}
              canModerate={workspace.role === 'owner' || workspace.role === 'admin'}
            />
          </div>
        </div>
        {/* Its right padding is the page's gutter, clear of the assistant's notch. */}
        <aside
          aria-label="Properties"
          className="space-y-1 border-t bg-muted/20 p-4 md:w-72 md:shrink-0 md:overflow-y-auto md:border-t-0 md:border-l md:pr-6"
        >
          <Property label="Status">
            <OptionSelect
              value={issue.state.id}
              onValueChange={(state_id) => save.mutate({ state_id })}
              options={states.map((s) => ({
                value: s.id,
                label: (
                  <span className="flex items-center gap-2">
                    <StateGlyph category={s.category} color={s.color} />
                    {s.name}
                  </span>
                ),
              }))}
              aria-label="State"
              className="w-full"
            />
          </Property>
          <Property label="Priority">
            <OptionSelect
              value={String(issue.priority)}
              onValueChange={(p) => save.mutate({ priority: Number(p) })}
              options={PRIORITY_LABEL.map((label, value) => ({
                value: String(value),
                label: (
                  <span className="flex items-center gap-2">
                    <PriorityGlyph priority={value} />
                    {label}
                  </span>
                ),
              }))}
              aria-label="Priority"
              className="w-full"
            />
          </Property>
          <Property label="Assignee">
            <OptionSelect
              value={issue.assignee?.user_id ?? NONE}
              onValueChange={(id) => save.mutate({ assignee_id: id === NONE ? null : id })}
              options={[
                { value: NONE, label: 'Unassigned' },
                ...(issue.assignee && !members.some((m) => m.user_id === issue.assignee?.user_id)
                  ? [{ value: issue.assignee.user_id, label: issue.assignee.name }]
                  : []),
                ...members.map((m) => ({ value: m.user_id, label: m.name })),
              ]}
              aria-label="Assignee"
              className="w-full"
            />
          </Property>
          <Property label="Labels">
            <LabelPicker
              workspace={workspace}
              selected={issue.labels}
              onChange={(label_ids) => save.mutate({ label_ids })}
            />
          </Property>
          <Property label="Project">
            <OptionSelect
              value={issue.project_id ?? NONE}
              onValueChange={(id) => save.mutate({ project_id: id === NONE ? null : id })}
              options={[
                { value: NONE, label: 'No project' },
                ...projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
              aria-label="Project"
              className="w-full"
            />
          </Property>
          <Property label="Due date">
            <Input
              type="date"
              aria-label="Due date"
              value={issue.due_date ?? ''}
              className="h-8 w-full text-[13px]"
              onChange={(e) => {
                const next = e.target.value || null;
                if (next !== issue.due_date) save.mutate({ due_date: next });
              }}
            />
          </Property>
          {(cycles.length > 0 || issue.cycle) && (
            <Property label="Cycle">
              <OptionSelect
                value={issue.cycle?.id ?? NONE}
                onValueChange={(id) => save.mutate({ cycle_id: id === NONE ? null : id })}
                options={[
                  { value: NONE, label: 'No cycle' },
                  ...cycles.map((c) => ({
                    value: c.id,
                    label: c.status === 'active' ? `${cycleLabel(c)} · active` : cycleLabel(c),
                  })),
                ]}
                aria-label="Cycle"
                className="w-full"
              />
            </Property>
          )}
          <Property label="GitHub">
            <GithubLink issue={issue} />
          </Property>
          <dl className="mt-3 space-y-1 border-t pt-3 text-xs text-muted-foreground">
            <div className="flex justify-between gap-2">
              <dt>Created</dt>
              <dd>{formatRelative(issue.created_at)}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt>Updated</dt>
              <dd>{formatRelative(issue.updated_at)}</dd>
            </div>
            {issue.completed_at && (
              <div className="flex justify-between gap-2">
                <dt>Closed</dt>
                <dd>{formatRelative(issue.completed_at)}</dd>
              </div>
            )}
          </dl>
        </aside>
      </div>
      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={`Delete ${issue.identifier}?`}
        description="The issue is removed. A graph created for it stays."
        confirmLabel="Delete issue"
        destructive
        onConfirm={() => remove.mutate()}
      />
    </div>
  );
}
