import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlusIcon, TagIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { LabelChip } from '@/components/custom-ui/issue-glyphs';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Label } from '@/schemas/issue';
import type { Workspace } from '@/schemas/workspace';

import { createLabel, deleteLabel, labelsQuery } from './api';

/** Colours offered for a new label, picked in turn so neighbours differ. */
const PALETTE = ['#ef4444', '#f59e0b', '#10b981', '#0ea5e9', '#6366f1', '#64748b'];

/**
 * The labels on an issue, with a menu to tick workspace labels on and off and to add a new one.
 * Guests see the labels but cannot add any; only admins remove a label from the workspace.
 */
export function LabelPicker({
  workspace,
  selected,
  onChange,
}: {
  workspace: Workspace;
  selected: Label[];
  onChange: (labelIds: string[]) => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const { data: labels = [] } = useQuery(labelsQuery(workspace.id));
  const chosen = new Set(selected.map((l) => l.id));
  const reload = () => queryClient.invalidateQueries({ queryKey: qk.issues.all });
  const add = useMutation({
    mutationFn: () =>
      createLabel(workspace.id, {
        name: name.trim(),
        color: PALETTE[labels.length % PALETTE.length]!,
      }),
    meta: { errorToast: false },
    onSuccess: (label) => {
      setName('');
      void reload();
      onChange([...chosen, label.id]);
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteLabel(workspace.id, id),
    onSuccess: reload,
  });
  const toggle = (id: string) => {
    const next = new Set(chosen);
    if (!next.delete(id)) next.add(id);
    onChange([...next]);
  };
  const canAdd = workspace.role !== 'guest';
  const canRemove = workspace.role === 'owner' || workspace.role === 'admin';

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {selected.map((label) => (
        <LabelChip key={label.id} name={label.name} color={label.color} />
      ))}
      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" className="h-6 gap-1.5 px-2 text-xs">
            <TagIcon className="size-3.5" />
            {selected.length === 0 ? 'Add label' : 'Labels'}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-64 space-y-2 p-2">
          {labels.length === 0 ? (
            <p className="px-1 text-[13px] text-muted-foreground">
              This workspace has no labels yet.
            </p>
          ) : (
            <ul className="max-h-56 space-y-0.5 overflow-y-auto">
              {labels.map((label) => (
                <li key={label.id} className="flex items-center gap-1">
                  <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-[13px] hover:bg-muted/60">
                    <Checkbox
                      checked={chosen.has(label.id)}
                      onCheckedChange={() => toggle(label.id)}
                    />
                    <span
                      aria-hidden
                      className="size-2 shrink-0 rounded-full"
                      style={{ backgroundColor: label.color }}
                    />
                    <span className="truncate">{label.name}</span>
                  </label>
                  {canRemove && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-6 text-muted-foreground hover:text-destructive"
                      aria-label={`Delete label ${label.name} from the workspace`}
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(label.id)}
                    >
                      <Trash2Icon className="size-3.5" />
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {canAdd && (
            <form
              className="flex gap-1.5 border-t pt-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (name.trim()) add.mutate();
              }}
            >
              <Input
                value={name}
                maxLength={40}
                placeholder="New label"
                aria-label="New label name"
                className="h-7 text-[13px]"
                onChange={(e) => setName(e.target.value)}
              />
              <Button
                type="submit"
                size="icon"
                variant="outline"
                className="size-7 shrink-0"
                aria-label="Create label"
                disabled={!name.trim() || add.isPending}
              >
                <PlusIcon className="size-3.5" />
              </Button>
            </form>
          )}
          {(add.error ?? remove.error) && (
            <p role="alert" className="px-1 text-xs text-destructive">
              {errorMessage(add.error ?? remove.error)}
            </p>
          )}
        </PopoverContent>
      </Popover>
    </div>
  );
}
