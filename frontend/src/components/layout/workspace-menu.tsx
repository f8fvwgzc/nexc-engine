import { useMutation, useQueryClient } from '@tanstack/react-query';
import { PlusIcon } from 'lucide-react';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { createWorkspace } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import type { Workspace } from '@/schemas/workspace';
import { useWorkspaceStore } from '@/stores/workspace-store';

/** Names a new workspace and opens it. */
export function NewWorkspaceDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const queryClient = useQueryClient();
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const create = useMutation({
    mutationFn: createWorkspace,
    meta: { errorToast: false, successMessage: 'Workspace created' },
    onSuccess: (workspace) => {
      // The new workspace is listed at once; the list is read again behind it.
      queryClient.setQueryData<Workspace[]>(qk.workspaces.all, (list) =>
        list ? [...list, workspace] : [workspace],
      );
      void queryClient.invalidateQueries({ queryKey: qk.workspaces.all });
      setCurrent(workspace.id);
      setName('');
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-md">
        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) create.mutate({ name: name.trim() });
          }}
        >
          <DialogHeader>
            <DialogTitle>New workspace</DialogTitle>
            <DialogDescription>
              A workspace is an organisation: its members, teams and everything they build.
            </DialogDescription>
          </DialogHeader>
          <Input
            autoFocus
            value={name}
            maxLength={80}
            placeholder="Acme Inc."
            aria-label="Workspace name"
            onChange={(e) => setName(e.target.value)}
          />
          {create.error && (
            <p role="alert" className="text-sm text-destructive">
              {errorMessage(create.error)}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim() || create.isPending}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The workspaces part of the account menu, as in Linear: the open workspace, which opens the list
 * of the member's workspaces to switch to and the way to a new one. It renders menu items only:
 * the account menu holds them, and the dialog `onCreate` opens lives beside that menu, since a
 * menu's content goes away as soon as it closes. `onSwitch` runs after the switch.
 */
export function WorkspaceSubmenu({
  onCreate,
  onSwitch,
}: {
  onCreate: () => void;
  onSwitch?: () => void;
}) {
  const { current, all } = useCurrentWorkspace();
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const navigate = useNavigate();
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger
        className="gap-2"
        aria-label={current ? `Switch workspace (${current.name}, ${current.role})` : undefined}
      >
        {/* The workspace's initial: the brand mark is in the rail. */}
        <span
          aria-hidden
          className="flex size-5 shrink-0 items-center justify-center rounded-[5px] bg-foreground/10 text-[11px] font-semibold"
        >
          {current?.name.trim().charAt(0).toUpperCase() || 'N'}
        </span>
        <span className="min-w-0 flex-1 truncate">{current?.name ?? 'Loading…'}</span>
        {current && (
          <span className="text-xs text-muted-foreground capitalize">{current.role}</span>
        )}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="max-w-72 min-w-56">
        <DropdownMenuLabel className="text-xs text-muted-foreground">Workspaces</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={current?.id ?? ''}
          onValueChange={(id) => {
            if (id === current?.id) return;
            setCurrent(id);
            // A graph of the previous workspace must not stay on screen.
            void navigate('/app');
            onSwitch?.();
          }}
        >
          {all.map((workspace) => (
            <DropdownMenuRadioItem key={workspace.id} value={workspace.id} className="gap-2">
              <span className="min-w-0 flex-1 truncate">{workspace.name}</span>
              <span className="text-xs text-muted-foreground">
                {workspace.member_count === 1 ? '1 member' : `${workspace.member_count} members`}
              </span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onCreate}>
          <PlusIcon />
          New workspace…
        </DropdownMenuItem>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}
