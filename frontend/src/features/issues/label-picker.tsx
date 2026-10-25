import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { PlusIcon, TagIcon, Trash2Icon } from 'lucide-react';
import { useState } from 'react';

import { IconButton } from '@/components/custom-ui/icon-button';
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
 * The labels on an issue as chips in one quiet button (or "Add label" without any), which opens
 * a menu to tick workspace labels on and off and to add a new one. Guests see the labels but
 * cannot add any; only admins remove a label from the workspace.
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
  const add = useMutation({
    mutationFn: () =>
      createLabel(workspace.id, {
        name: name.trim(),
        color: PALETTE[labels.length % PALETTE.length]!,
      }),
    meta: { errorToast: false },
    onSuccess: (label) => {
      setName('');
      // Only the workspace's labels changed; ticking it on saves the issue, which reads it again.
      void queryClient.invalidateQueries({ queryKey: qk.issues.labels(workspace.id) });
      onChange([...chosen, label.id]);
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => deleteLabel(workspace.id, id),
    // It came off every issue that had it: lists and issues are read again, not just the labels.
    onSuccess: () => queryClient.invalidateQueries({ queryKey: qk.issues.all }),
  });
  const toggle = (id: string) => {
    const next = new Set(chosen);
    if (!next.delete(id)) next.add(id);
    onChange([...next]);
  };
  const canAdd = workspace.role !== 'guest';
  const canRemove = workspace.role === 'owner' || workspace.role === 'admin';

  return (
    <Popover>
      <PopoverTrigger asChild>
        {selected.length === 0 ? (
          <Button
            variant="ghost"
            size="sm"
            className="h-7 w-full justify-start gap-2 px-1.5 text-[13px] font-normal text-muted-foreground hover:bg-muted"
          >
            <TagIcon />
            Add label
          </Button>
        ) : (
          <Button
            variant="ghost"
            size="sm"
            aria-label={`Labels: ${selected.map((l) => l.name).join(', ')}`}
            className="h-auto min-h-7 w-full flex-wrap justify-start gap-1 px-1 py-1 font-normal hover:bg-muted"
          >
            {selected.map((label) => (
              <LabelChip key={label.id} name={label.name} color={label.color} />
            ))}
            <PlusIcon className="size-3.5 text-muted-foreground" />
          </Button>
        )}
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
                  <IconButton
                    size="icon"
                    className="size-6 text-muted-foreground hover:text-destructive"
                    label={`Delete label ${label.name} from the workspace`}
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(label.id)}
                  >
                    <Trash2Icon className="size-3.5" />
                  </IconButton>
                )}
              </li>
            ))}
          </ul>
        )}
        {canAdd && (
          <form
            className="flex gap-1.5"
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
            <IconButton
              type="submit"
              size="icon"
              variant="outline"
              className="size-7 shrink-0"
              label="Create label"
              disabled={!name.trim() || add.isPending}
            >
              <PlusIcon className="size-3.5" />
            </IconButton>
          </form>
        )}
        {(add.error ?? remove.error) && (
          <p role="alert" className="px-1 text-xs text-destructive">
            {errorMessage(add.error ?? remove.error)}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}
