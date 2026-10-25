import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BoxIcon,
  CalendarIcon,
  HashIcon,
  IterationCwIcon,
  MoreHorizontalIcon,
  NetworkIcon,
  PanelRightCloseIcon,
  Trash2Icon,
  WaypointsIcon,
  XIcon,
} from 'lucide-react';
import { m } from 'motion/react';
import { useState, useSyncExternalStore, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { toast } from 'sonner';

import { CopyButton } from '@/components/custom-ui/copy-button';
import { GithubChip, GithubMark } from '@/components/custom-ui/github-chip';
import { IconButton } from '@/components/custom-ui/icon-button';
import {
  PersonGlyph,
  PriorityGlyph,
  ProjectStatusGlyph,
  StateGlyph,
} from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import { Calendar } from '@/components/ui/calendar';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Skeleton } from '@/components/ui/skeleton';
import { ontologyStatusQuery } from '@/features/ontology/api';
import { membersQuery } from '@/features/workspaces/api';
import { formatDateTime, formatRelative } from '@/lib/format';
import { githubUrlError } from '@/lib/github';
import { MotionPresence, spring } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { cycleLabel } from '@/schemas/cycle';
import { PRIORITY_LABEL, type Issue, type IssueInput } from '@/schemas/issue';
import { entityPath, ontoEntity } from '@/schemas/ontology';
import type { Workspace } from '@/schemas/workspace';

import { cyclesQuery, projectsQuery, statesQuery, updateIssue } from './api';
import { dueLabel, localToday } from './due';
import { LabelPicker } from './label-picker';
import { issuePath } from './links';

const NONE = '__none__';

/** A property's value: a borderless button that shows it, like Linear's (no box, no chevron). */
const GHOST =
  'h-7 w-full min-w-0 justify-start gap-2 px-1.5 text-[13px] font-normal hover:bg-muted';

/** Below `md` the panel follows the main column instead of standing beside it. */
const PHONE = '(max-width: 767px)';

/** Whether the screen is phone-sized, read in the same render that lays the panel out. */
function usePhone(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const query = window.matchMedia(PHONE);
      query.addEventListener('change', onChange);
      return () => query.removeEventListener('change', onChange);
    },
    () => window.matchMedia(PHONE).matches,
    () => false,
  );
}

/** One property of the issue: its name beside its value. */
function PropertyRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[6rem_minmax(0,1fr)] items-start">
      <p className="flex h-8 items-center truncate text-xs text-muted-foreground">{label}</p>
      <div className="flex min-h-8 min-w-0 items-center">{children}</div>
    </div>
  );
}

interface Choice {
  value: string;
  label: string;
  icon?: ReactNode;
}

/**
 * A property's value that opens a searchable list of what it can be, with a tick on the current
 * one. Picking another saves it; picking the same one only closes the list.
 */
function PropertyPicker({
  label,
  search,
  value,
  text,
  choices,
  onPick,
  children,
  className,
}: {
  /** The property, for the button's name ("Status: In progress"). */
  label: string;
  /** The search field's placeholder ("Change status…"). */
  search: string;
  value: string;
  /** The current value as words, for the button's name. */
  text: string;
  choices: Choice[];
  onPick: (value: string) => void;
  /** What the button shows: the value's glyph and name. */
  children: ReactNode;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          aria-label={`${label}: ${text}`}
          className={cn(GHOST, className)}
        >
          {children}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-60 p-0">
        <Command>
          <CommandInput placeholder={search} aria-label={search} />
          <CommandList>
            <CommandEmpty>Nothing matches.</CommandEmpty>
            <CommandGroup>
              {choices.map((choice) => (
                <CommandItem
                  key={choice.value}
                  value={`${choice.label} ${choice.value}`}
                  data-checked={choice.value === value}
                  className="text-[13px]"
                  onSelect={() => {
                    setOpen(false);
                    if (choice.value !== value) onPick(choice.value);
                  }}
                >
                  {/* The glyphs name themselves; the label already says it. */}
                  {choice.icon && (
                    <span aria-hidden className="contents">
                      {choice.icon}
                    </span>
                  )}
                  <span className="truncate">{choice.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/** A calendar day (`YYYY-MM-DD`) as midnight of that day where the reader is. */
const dayDate = (day: string) => new Date(`${day}T00:00:00`);

/** When the issue is due, picked on a calendar; red once the day has passed and it is open. */
function DueDate({ issue, onPick }: { issue: Issue; onPick: (due: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const closed = issue.state.category === 'completed' || issue.state.category === 'canceled';
  const due = issue.due_date ? dueLabel(issue.due_date, localToday(), !closed) : null;
  const pick = (next: string | null) => {
    setOpen(false);
    if (next !== issue.due_date) onPick(next);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          aria-label={`Due date: ${issue.due_date ?? 'none'}`}
          title={issue.due_date ? `Due ${issue.due_date}` : undefined}
          className={cn(GHOST, due?.late && 'text-destructive', !due && 'text-muted-foreground')}
        >
          <CalendarIcon />
          <span className="truncate tabular-nums">{due ? due.text : 'Set due date'}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-auto gap-0 p-0">
        <Calendar
          mode="single"
          selected={issue.due_date ? dayDate(issue.due_date) : undefined}
          defaultMonth={issue.due_date ? dayDate(issue.due_date) : undefined}
          onSelect={(date) => pick(date ? localToday(date) : null)}
        />
        {issue.due_date && (
          <div className="px-2 pb-2">
            <Button
              variant="ghost"
              size="sm"
              className="w-full text-muted-foreground"
              onClick={() => pick(null)}
            >
              Clear due date
            </Button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * The GitHub issue or pull request the issue is linked to, with a button that removes the link;
 * without one, a quiet button that opens a field to paste a URL, saved when it is left or on
 * Enter. A URL the server refuses stays in the field with the server's reason under it.
 */
function GithubField({
  issue,
  onSaved,
}: {
  issue: Issue;
  onSaved: (updated: Issue, body: Partial<IssueInput>) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const link = useMutation({
    mutationFn: (github_url: string | null) => updateIssue(issue.id, { github_url }),
    meta: { errorToast: false },
    onSuccess: (updated, github_url) => {
      setDraft('');
      setEditing(false);
      onSaved(updated, { github_url });
    },
  });
  const failure = link.error && (
    <p role="alert" className="px-1.5 text-xs text-destructive">
      {githubUrlError(link.error)}
    </p>
  );
  if (issue.github_url) {
    return (
      <div className="w-full min-w-0 space-y-1">
        <div className="group/github flex min-w-0 items-center gap-0.5">
          <GithubChip url={issue.github_url} form="repo" variant="plain" className="min-w-0" />
          <IconButton
            label="Remove the GitHub link"
            size="icon-xs"
            className="text-muted-foreground"
            disabled={link.isPending}
            onClick={() => link.mutate(null)}
          >
            <XIcon />
          </IconButton>
        </div>
        {failure}
      </div>
    );
  }
  if (!editing) {
    return (
      <Button
        variant="ghost"
        size="sm"
        className={cn(GHOST, 'text-muted-foreground')}
        onClick={() => setEditing(true)}
      >
        <GithubMark className="size-3.5" />
        <span className="truncate">Link GitHub issue or PR</span>
      </Button>
    );
  }
  const commit = () => {
    const url = draft.trim();
    if (!url) {
      if (!link.isPending) setEditing(false);
      return;
    }
    // The same refused URL is not sent again until it is changed.
    if (link.isPending || (link.isError && link.variables === url)) return;
    link.mutate(url);
  };
  return (
    <div className="w-full space-y-1">
      <input
        autoFocus
        value={draft}
        inputMode="url"
        placeholder="Paste an issue or PR URL"
        aria-label="GitHub issue or PR URL"
        aria-invalid={link.isError || undefined}
        className="h-7 w-full min-w-0 rounded-md bg-muted/50 px-1.5 text-[13px] outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/50 aria-invalid:ring-2 aria-invalid:ring-destructive/40"
        onChange={(e) => {
          setDraft(e.target.value);
          if (link.isError) link.reset();
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          } else if (e.key === 'Escape') {
            e.preventDefault();
            setDraft('');
            link.reset();
            setEditing(false);
          }
        }}
      />
      {failure}
    </div>
  );
}

/**
 * The rest of what can be done with the issue: open its graph, see it on the workspace map
 * (asked only once the menu is wanted, so the page does not wait on onto-intelli), copy its ID.
 */
function MoreMenu({ workspace, issue }: { workspace: Workspace; issue: Issue }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const status = { ...ontologyStatusQuery(workspace.id), retry: false } as const;
  const { data: onto } = useQuery({ ...status, enabled: open });
  const warm = () => void queryClient.prefetchQuery(status);
  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(issue.identifier);
      toast.success(`Copied ${issue.identifier}`);
    } catch {
      toast.error('Clipboard is not available');
    }
  };
  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <IconButton
          label="More actions"
          className="text-muted-foreground hover:text-foreground"
          onPointerEnter={warm}
          onFocus={warm}
        >
          <MoreHorizontalIcon />
        </IconButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-44">
        {issue.graph_id && (
          <DropdownMenuItem asChild>
            <Link to={`/app/graphs/${issue.graph_id}`}>
              <NetworkIcon />
              Open graph
            </Link>
          </DropdownMenuItem>
        )}
        {onto && onto.mode !== 'off' && (
          <DropdownMenuItem asChild>
            <Link to={entityPath(ontoEntity('issue', issue.id))}>
              <WaypointsIcon />
              On the map
            </Link>
          </DropdownMenuItem>
        )}
        <DropdownMenuItem onSelect={() => void copyId()}>
          <HashIcon />
          Copy ID
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** When something happened to the issue, as a plain row under its properties. */
function MetaRow({ label, at }: { label: string; at: string }) {
  return (
    <div className="grid min-h-7 grid-cols-[6rem_minmax(0,1fr)] items-center">
      <dt>{label}</dt>
      <dd className="truncate px-1.5" title={formatDateTime(at)}>
        {formatRelative(at)}
      </dd>
    </div>
  );
}

/**
 * The issue's properties beside its main column, laid out like Linear's: a name and a quiet
 * value per row, each value a button that opens a searchable picker and saves what is picked.
 * The header copies the link, deletes the issue, holds the rarer actions and closes the panel,
 * which opens and closes with a spring (stacked under the main column on a phone). Its right
 * padding is the page's gutter, clear of the assistant's notch.
 */
export function IssueProperties({
  workspace,
  issue,
  open,
  onClose,
  save,
  onSaved,
  onDelete,
}: {
  workspace: Workspace;
  issue: Issue;
  open: boolean;
  onClose: () => void;
  /** Saves a change to the issue. */
  save: (body: Partial<IssueInput>) => void;
  /** Shows a copy of the issue saved here outside `save` (the GitHub field saves on its own). */
  onSaved: (updated: Issue, body: Partial<IssueInput>) => void;
  onDelete: () => void;
}) {
  const phone = usePhone();
  // What the pickers offer, read while the panel is open.
  const { data: states = [] } = useQuery({
    ...statesQuery(workspace.id, issue.team_id),
    enabled: open,
  });
  const projects = useQuery({ ...projectsQuery(workspace.id), enabled: open });
  const { data: cycles = [] } = useQuery({
    ...cyclesQuery(workspace.id, issue.team_id),
    enabled: open,
  });
  // Guests cannot list the workspace's members; they keep whoever is assigned.
  const { data: members = [] } = useQuery({
    ...membersQuery(workspace.id),
    enabled: workspace.role !== 'guest' && open,
  });
  const project = projects.data?.find((p) => p.id === issue.project_id);
  const hidden = phone ? { height: 0, opacity: 0 } : { width: 0, opacity: 0 };

  const stateChoices = (states.length > 0 ? states : [issue.state]).map((s) => ({
    value: s.id,
    label: s.name,
    icon: <StateGlyph category={s.category} color={s.color} />,
  }));
  // Whoever is assigned stays a choice even when the member list does not have them.
  const people =
    issue.assignee && !members.some((mb) => mb.user_id === issue.assignee?.user_id)
      ? [issue.assignee, ...members]
      : members;
  const assigneeChoices: Choice[] = [
    { value: NONE, label: 'Unassigned', icon: <PersonGlyph name={undefined} /> },
    ...people.map((person) => ({
      value: person.user_id,
      label: person.name,
      icon: <PersonGlyph name={person.name} />,
    })),
  ];
  const projectChoices: Choice[] = [
    { value: NONE, label: 'No project', icon: <BoxIcon className="text-muted-foreground" /> },
    ...(projects.data ?? []).map((p) => ({
      value: p.id,
      label: p.name,
      icon: <ProjectStatusGlyph status={p.status} />,
    })),
  ];
  const cycleChoices: Choice[] = [
    { value: NONE, label: 'No cycle' },
    ...cycles.map((c) => ({
      value: c.id,
      label: c.status === 'active' ? `${cycleLabel(c)} · active` : cycleLabel(c),
    })),
  ].map((choice) => ({
    ...choice,
    icon: <IterationCwIcon className="text-muted-foreground" />,
  }));

  return (
    <MotionPresence>
      {open && (
        <m.aside
          key="properties"
          aria-label="Properties"
          initial={hidden}
          animate={phone ? { height: 'auto', opacity: 1 } : { width: '18rem', opacity: 1 }}
          exit={hidden}
          transition={spring}
          className="shrink-0 overflow-hidden md:overflow-y-auto"
        >
          <div className="w-full px-4 pt-2 pb-8 sm:px-8 md:w-72 md:pr-6 md:pl-3">
            <div className="flex h-10 items-center gap-0.5">
              <h2 className="mr-auto pl-0 text-[13px] font-medium text-muted-foreground">
                Properties
              </h2>
              <CopyButton
                value={`${window.location.origin}${issuePath(issue.id)}`}
                label="Copy link"
              />
              <IconButton
                className="text-muted-foreground hover:text-destructive"
                label={`Delete ${issue.identifier}`}
                onClick={onDelete}
              >
                <Trash2Icon />
              </IconButton>
              <MoreMenu workspace={workspace} issue={issue} />
              <IconButton
                label="Close properties"
                className="text-muted-foreground hover:text-foreground"
                onClick={onClose}
              >
                <PanelRightCloseIcon />
              </IconButton>
            </div>
            <div className="space-y-0.5">
              <PropertyRow label="Status">
                <PropertyPicker
                  label="Status"
                  search="Change status…"
                  value={issue.state.id}
                  text={issue.state.name}
                  choices={stateChoices}
                  onPick={(state_id) => save({ state_id })}
                >
                  <StateGlyph category={issue.state.category} color={issue.state.color} />
                  <span className="truncate">{issue.state.name}</span>
                </PropertyPicker>
              </PropertyRow>
              <PropertyRow label="Priority">
                <PropertyPicker
                  label="Priority"
                  search="Set priority…"
                  value={String(issue.priority)}
                  text={PRIORITY_LABEL[issue.priority] ?? PRIORITY_LABEL[0]}
                  choices={PRIORITY_LABEL.map((name, value) => ({
                    value: String(value),
                    label: name,
                    icon: <PriorityGlyph priority={value} />,
                  }))}
                  onPick={(p) => save({ priority: Number(p) })}
                  className={issue.priority === 0 ? 'text-muted-foreground' : undefined}
                >
                  <PriorityGlyph priority={issue.priority} />
                  <span className="truncate">
                    {PRIORITY_LABEL[issue.priority] ?? PRIORITY_LABEL[0]}
                  </span>
                </PropertyPicker>
              </PropertyRow>
              <PropertyRow label="Assignee">
                <PropertyPicker
                  label="Assignee"
                  search="Assign to…"
                  value={issue.assignee?.user_id ?? NONE}
                  text={issue.assignee?.name ?? 'Unassigned'}
                  choices={assigneeChoices}
                  onPick={(id) => save({ assignee_id: id === NONE ? null : id })}
                  className={issue.assignee ? undefined : 'text-muted-foreground'}
                >
                  <PersonGlyph name={issue.assignee?.name} />
                  <span className="truncate">{issue.assignee?.name ?? 'Unassigned'}</span>
                </PropertyPicker>
              </PropertyRow>
              <PropertyRow label="Labels">
                <LabelPicker
                  workspace={workspace}
                  selected={issue.labels}
                  onChange={(label_ids) => save({ label_ids })}
                />
              </PropertyRow>
              <PropertyRow label="Project">
                <PropertyPicker
                  label="Project"
                  search="Move to project…"
                  value={issue.project_id ?? NONE}
                  text={project?.name ?? (issue.project_id ? 'Unknown project' : 'No project')}
                  choices={projectChoices}
                  onPick={(id) => save({ project_id: id === NONE ? null : id })}
                  className={issue.project_id ? undefined : 'text-muted-foreground'}
                >
                  {project ? (
                    <ProjectStatusGlyph status={project.status} />
                  ) : (
                    <BoxIcon className="text-muted-foreground" />
                  )}
                  {issue.project_id && projects.isPending ? (
                    <Skeleton className="h-3.5 w-24">
                      <span className="sr-only">Loading the project</span>
                    </Skeleton>
                  ) : (
                    <span className="truncate">
                      {project?.name ?? (issue.project_id ? 'Unknown project' : 'No project')}
                    </span>
                  )}
                </PropertyPicker>
              </PropertyRow>
              {(cycles.length > 0 || issue.cycle) && (
                <PropertyRow label="Cycle">
                  <PropertyPicker
                    label="Cycle"
                    search="Move to cycle…"
                    value={issue.cycle?.id ?? NONE}
                    text={issue.cycle ? cycleLabel(issue.cycle) : 'No cycle'}
                    choices={cycleChoices}
                    onPick={(id) => save({ cycle_id: id === NONE ? null : id })}
                    className={issue.cycle ? undefined : 'text-muted-foreground'}
                  >
                    <IterationCwIcon className="text-muted-foreground" />
                    <span className="truncate">
                      {issue.cycle ? cycleLabel(issue.cycle) : 'No cycle'}
                    </span>
                  </PropertyPicker>
                </PropertyRow>
              )}
              <PropertyRow label="Due date">
                <DueDate issue={issue} onPick={(due_date) => save({ due_date })} />
              </PropertyRow>
              <PropertyRow label="GitHub">
                <GithubField issue={issue} onSaved={onSaved} />
              </PropertyRow>
            </div>
            <dl className="mt-4 text-xs text-muted-foreground">
              <MetaRow label="Created" at={issue.created_at} />
              <MetaRow label="Updated" at={issue.updated_at} />
              {issue.completed_at && <MetaRow label="Closed" at={issue.completed_at} />}
            </dl>
          </div>
        </m.aside>
      )}
    </MotionPresence>
  );
}

/** The panel's frame while the issue loads: its header and a row per property. */
export function PropertiesSkeleton() {
  return (
    <div className="w-full shrink-0 px-4 pt-2 pb-8 sm:px-8 md:w-72 md:pr-6 md:pl-3">
      <div className="flex h-10 items-center">
        <Skeleton className="h-3.5 w-20" />
      </div>
      {Array.from({ length: 7 }, (_, i) => (
        <div key={i} className="grid h-8 grid-cols-[6rem_minmax(0,1fr)] items-center">
          <Skeleton className="h-3 w-14" />
          <Skeleton className="h-4 w-28" />
        </div>
      ))}
    </div>
  );
}
