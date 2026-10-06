import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { StateGlyph } from '@/components/custom-ui/issue-glyphs';
import { OptionSelect } from '@/components/custom-ui/option-select';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { IssueState, StateCategory } from '@/schemas/issue';
import type { Team, Workspace } from '@/schemas/workspace';

import { createState, deleteState, statesQuery, updateState, type StateInput } from './api';

const CATEGORIES: { value: StateCategory; label: string }[] = [
  { value: 'backlog', label: 'Backlog' },
  { value: 'unstarted', label: 'Not started' },
  { value: 'started', label: 'In progress' },
  { value: 'completed', label: 'Done' },
  { value: 'canceled', label: 'Canceled' },
];

/** One editable state. Text commits on blur so a rename is one request, not one per key. */
function StateRow({
  state,
  editable,
  first,
  last,
  onChange,
  onMove,
  onDelete,
}: {
  state: IssueState;
  editable: boolean;
  first: boolean;
  last: boolean;
  onChange: (patch: Partial<StateInput>) => void;
  onMove: (direction: -1 | 1) => void;
  onDelete: () => void;
}) {
  const [name, setName] = useState(state.name);
  const commitName = () => {
    const next = name.trim();
    if (next && next !== state.name) onChange({ name: next });
    else setName(state.name);
  };
  return (
    <li className="flex items-center gap-2 py-1.5">
      <StateGlyph category={state.category} color={state.color} />
      <input
        type="color"
        value={state.color}
        disabled={!editable}
        aria-label={`Colour of ${state.name}`}
        onChange={(e) => onChange({ color: e.target.value })}
        className="size-7 shrink-0 cursor-pointer rounded border bg-transparent p-0.5 disabled:cursor-default"
      />
      <Input
        value={name}
        maxLength={40}
        disabled={!editable}
        aria-label="State name"
        onChange={(e) => setName(e.target.value)}
        onBlur={commitName}
        onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
        className="h-8 min-w-0 flex-1"
      />
      <OptionSelect
        value={state.category}
        onValueChange={(category) => category !== state.category && onChange({ category })}
        options={CATEGORIES}
        disabled={!editable}
        aria-label={`Meaning of ${state.name}`}
        className="h-8 w-36 shrink-0"
      />
      {editable && (
        <>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={first}
            aria-label={`Move ${state.name} up`}
            onClick={() => onMove(-1)}
          >
            <ArrowUpIcon />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={last}
            aria-label={`Move ${state.name} down`}
            onClick={() => onMove(1)}
          >
            <ArrowDownIcon />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-destructive"
            aria-label={`Delete ${state.name}`}
            onClick={onDelete}
          >
            <Trash2Icon />
          </Button>
        </>
      )}
    </li>
  );
}

/** The workflow of a team: its states, what each means, and their order. */
export function WorkflowDialog({
  workspace,
  team,
  editable,
  open,
  onClose,
}: {
  workspace: Workspace;
  team: Team;
  editable: boolean;
  open: boolean;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  const { data: states = [] } = useQuery({
    ...statesQuery(workspace.id, team.id),
    enabled: open,
  });
  const [newName, setNewName] = useState('');
  const [newCategory, setNewCategory] = useState<StateCategory>('started');
  // States are shown on issues everywhere, so every change refreshes issue data too.
  const refresh = () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
  const change = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: Partial<StateInput> }) =>
      updateState(workspace.id, team.id, id, patch),
    meta: { errorToast: false },
    onSettled: refresh,
  });
  const add = useMutation({
    mutationFn: () =>
      createState(workspace.id, team.id, {
        name: newName.trim(),
        category: newCategory,
        color: '#64748b',
      }),
    meta: { errorToast: false },
    onSuccess: () => setNewName(''),
    onSettled: refresh,
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteState(workspace.id, team.id, id),
    meta: { errorToast: false },
    onSettled: refresh,
  });
  // Swapping two rows means giving each the other's place; positions are renumbered so that
  // states which shared a position (older data) end up distinct.
  const move = (index: number, direction: -1 | 1) => {
    const order = [...states];
    const other = index + direction;
    const [a, b] = [order[index], order[other]];
    if (!a || !b) return;
    order[index] = b;
    order[other] = a;
    order.forEach((state, position) => {
      if (state.position !== position) change.mutate({ id: state.id, patch: { position } });
    });
  };
  const error = change.error ?? add.error ?? remove.error;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Workflow · {team.name}</DialogTitle>
          <DialogDescription>
            The states an issue of {team.key} moves through. Names, colours and order are yours; the
            meaning tells Nexc whether an issue is open or closed.
            {editable ? '' : ' Only team owners and workspace admins can change it.'}
          </DialogDescription>
        </DialogHeader>
        <ul className="max-h-[50svh] divide-y overflow-y-auto pr-1">
          {states.map((state, index) => (
            <StateRow
              key={`${state.id}:${state.name}`}
              state={state}
              editable={editable}
              first={index === 0}
              last={index === states.length - 1}
              onChange={(patch) => change.mutate({ id: state.id, patch })}
              onMove={(direction) => move(index, direction)}
              onDelete={() => remove.mutate(state.id)}
            />
          ))}
        </ul>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {errorMessage(error)}
          </p>
        )}
        {editable && (
          <form
            className="flex items-center gap-2 border-t pt-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (newName.trim()) add.mutate();
            }}
          >
            <Input
              value={newName}
              maxLength={40}
              placeholder="New state, e.g. QA"
              aria-label="New state name"
              onChange={(e) => setNewName(e.target.value)}
              className="h-8 min-w-0 flex-1"
            />
            <OptionSelect
              value={newCategory}
              onValueChange={setNewCategory}
              options={CATEGORIES}
              aria-label="Meaning of the new state"
              className="h-8 w-36 shrink-0"
            />
            <Button type="submit" size="sm" disabled={!newName.trim() || add.isPending}>
              <PlusIcon />
              Add
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
