import { useMutation, useQueryClient } from '@tanstack/react-query';
import { CheckIcon, ChevronsUpDownIcon, PlusIcon } from 'lucide-react';
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from '@/components/ui/sidebar';
import { createWorkspace } from '@/features/workspaces/api';
import { useCurrentWorkspace } from '@/features/workspaces/use-current-workspace';
import { errorMessage } from '@/lib/api/errors';
import { qk } from '@/lib/query-keys';
import { useWorkspaceStore } from '@/stores/workspace-store';

import { BrandMark } from './brand-mark';

function NewWorkspaceDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const queryClient = useQueryClient();
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const create = useMutation({
    mutationFn: createWorkspace,
    meta: { errorToast: false, successMessage: 'Workspace created' },
    onSuccess: async (workspace) => {
      await queryClient.invalidateQueries({ queryKey: qk.workspaces.all });
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

/** Sidebar header: the open workspace, and the way to another one. */
export function WorkspaceSwitcher() {
  const { current, all } = useCurrentWorkspace();
  const setCurrent = useWorkspaceStore((s) => s.setCurrent);
  const { isMobile } = useSidebar();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              aria-label="Switch workspace"
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <BrandMark className="size-8 shrink-0" />
              <div className="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
                <span className="truncate font-semibold">{current?.name ?? 'Nexc'}</span>
                <span className="truncate text-xs text-muted-foreground capitalize">
                  {current ? current.role : 'Loading…'}
                </span>
              </div>
              <ChevronsUpDownIcon className="ml-auto size-4 group-data-[collapsible=icon]:hidden" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-(--radix-dropdown-menu-trigger-width) min-w-60 rounded-lg"
            side={isMobile ? 'bottom' : 'right'}
            align="start"
            sideOffset={4}
          >
            <DropdownMenuLabel className="text-xs text-muted-foreground">
              Workspaces
            </DropdownMenuLabel>
            {all.map((workspace) => (
              <DropdownMenuItem
                key={workspace.id}
                onSelect={() => {
                  if (workspace.id === current?.id) return;
                  setCurrent(workspace.id);
                  // A graph of the previous workspace must not stay on screen.
                  void navigate('/app');
                }}
              >
                <span className="min-w-0 flex-1 truncate">{workspace.name}</span>
                <span className="text-xs text-muted-foreground">
                  {workspace.member_count === 1 ? '1 member' : `${workspace.member_count} members`}
                </span>
                {workspace.id === current?.id && <CheckIcon className="size-4" aria-label="Open" />}
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => setCreating(true)}>
              <PlusIcon />
              New workspace
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
      <NewWorkspaceDialog open={creating} onClose={() => setCreating(false)} />
    </SidebarMenu>
  );
}
